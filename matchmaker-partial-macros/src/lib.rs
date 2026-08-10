//! `#[partial(...)]` attribute macro backing the `matchmaker-partial` crate.
//!
//! For a struct `Foo` it generates a "partial" mirror `PartialFoo` whose fields
//! are wrapped in `Option`, plus an `Apply` impl that moves partial values into
//! the original struct, and (optionally) `Set` and `Merge` impls.
//!
//! Note: this crate is a no-op when the `partial` feature is disabled — the
//! `#[partial]` attributes are stripped and the input is passed through.

use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use std::collections::HashSet;
use syn::{
    Attribute, Fields, GenericArgument, GenericParam, Ident, ItemStruct, LitStr, Meta, Path,
    PathArguments, Result as SynResult, Token, Type,
    ext::IdentExt,
    parse::{Parse, Parser},
    parse_macro_input,
    spanned::Spanned,
};

#[proc_macro_attribute]
pub fn partial(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemStruct);

    // Feature-disabled: strip the control attributes and pass the struct
    // through untouched.
    if !cfg!(feature = "partial") {
        input.attrs.retain(|attr| !attr.path().is_ident("partial"));
        if let Fields::Named(fields) = &mut input.fields {
            for field in &mut fields.named {
                field.attrs.retain(|attr| !attr.path().is_ident("partial"));
            }
        }
        return quote!(#input).into();
    }

    let struct_ident = input.ident.clone();
    let partial_ident = format_ident!("Partial{}", struct_ident);

    // ---- 1. Parse top-level `#[partial(...)]` arguments ----
    let options = match parse_struct_options(attr) {
        Ok(options) => options,
        Err(e) => return e.to_compile_error().into(),
    };

    // ---- 2. Assemble the attribute list of the generated `Partial*` struct ----
    let final_attrs = build_partial_struct_attrs(&input, &options);

    // ---- 3. Process fields ----
    let generic_idents = struct_generic_idents(&input);
    let fields = match &mut input.fields {
        Fields::Named(fields) => &mut fields.named,
        _ => {
            return syn::Error::new(
                struct_ident.span(),
                "Partial only supports structs with named fields",
            )
            .to_compile_error()
            .into();
        }
    };

    let ctx = FieldCtx {
        struct_ident: &struct_ident,
        struct_recurse: options.recurse,
        struct_unwrap: options.unwrap,
        no_field_mirror: options.no_field_mirror,
        generic_idents: &generic_idents,
    };

    let mut partial_field_defs = Vec::new();
    let mut apply_field_stmts = Vec::new();
    let mut merge_field_stmts = Vec::new();
    let mut clear_field_stmts = Vec::new();
    let mut set_field_arms = Vec::new();
    let mut flattened_field_targets = Vec::new();
    let mut used_idents = HashSet::new();
    // Custom-deserializer wrapper fns generated for mirrored fields, emitted
    // next to the partial struct. (See `process_serde_attr`.)
    let mut deserializer_wrappers = Vec::new();

    for field in fields.iter_mut() {
        // Named fields only (checked above), so the ident always exists.
        let raw_ident = field
            .ident
            .as_ref()
            .expect("Partial only supports structs with named fields")
            .clone();

        // Parse `#[partial(...)]` / `#[serde(...)]` attributes; may collect
        // errors (reported at the end) and wrappers (promoted only if the
        // field survives the `skip` check below).
        let mut state = parse_field_attrs(field, &ctx);

        let field_ty = &field.ty;
        let field_vis = &field.vis;
        let is_opt = is_option(field_ty);

        if let Some(err) = drain_errors(&mut state.errors) {
            return err.to_compile_error().into();
        }
        if state.options.skip {
            continue;
        }
        deserializer_wrappers.extend(std::mem::take(&mut state.wrappers));

        if state.options.set == SetMode::Sequence
            && state
                .options
                .recurse_override
                .as_ref()
                .is_some_and(|o| o.is_some())
        {
            return syn::Error::new(
                field.span(),
                "cannot use 'recurse' and 'set = \"sequence\"' on the same field",
            )
            .to_compile_error()
            .into();
        }

        let inner_ty = if is_opt {
            extract_inner_type_from_option(field_ty)
        } else {
            field_ty
        };
        let should_recurse = should_recurse_field(&ctx, &state.options);

        let codegen = match (|| -> SynResult<FieldCodegen> {
            match get_collection_info(inner_ty)? {
                Some(coll) => codegen_collection_field(
                    &state,
                    &raw_ident,
                    field_ty,
                    is_opt,
                    should_recurse,
                    &coll,
                ),
                None => codegen_leaf_field(
                    &state,
                    &raw_ident,
                    field_ty,
                    inner_ty,
                    is_opt,
                    should_recurse,
                ),
            }
        })() {
            Ok(codegen) => codegen,
            Err(e) => return e.to_compile_error().into(),
        };

        if let Some(arm) = codegen.set_arm {
            set_field_arms.push(arm);
        }
        if let Some(target) = codegen.flattened_target {
            flattened_field_targets.push(target);
        }
        apply_field_stmts.push(codegen.apply);
        merge_field_stmts.push(codegen.merge);
        clear_field_stmts.push(codegen.clear);

        find_idents_in_tokens(codegen.ty.clone(), &mut used_idents);
        let mirror_attrs = &state.mirror_attrs;
        let ty = &codegen.ty;
        partial_field_defs.push(quote! {
            #(#mirror_attrs)* #field_vis #raw_ident: #ty
        });
    }

    // ---- 4. Drop generics that no partial field references ----
    let mut partial_generics = input.generics.clone();
    partial_generics.params = partial_generics
        .params
        .into_iter()
        .filter(|param| match param {
            GenericParam::Type(t) => used_idents.contains(&t.ident),
            GenericParam::Lifetime(l) => used_idents.contains(&l.lifetime.ident),
            GenericParam::Const(c) => used_idents.contains(&c.ident),
        })
        .collect();

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let (p_impl_generics, p_ty_generics, p_where_clause) = partial_generics.split_for_impl();

    // ---- 5. Optional `Set` implementation (path-based partial updates) ----
    let path_setter_impl = if options.path_setter {
        quote! {
            impl #p_impl_generics matchmaker_partial::Set for #partial_ident #p_ty_generics #p_where_clause {
                fn set(&mut self, path: &[String], val: &[String]) -> Result<(), matchmaker_partial::PartialSetError> {
                    let (head, tail) = path.split_first().ok_or_else(|| {
                        matchmaker_partial::PartialSetError::EarlyEnd("root".to_string())
                    })?;

                    match head.as_str() {
                        #(#set_field_arms)*
                        _ => {
                            #(
                                match matchmaker_partial::Set::set(#flattened_field_targets, path, val) {
                                    Err(matchmaker_partial::PartialSetError::Missing(_)) => {}
                                    x => return x,
                                }
                            )*
                            Err(matchmaker_partial::PartialSetError::Missing(head.clone()))
                        }
                    }
                }
            }
        }
    } else {
        quote! {}
    };

    // ---- 6. Optional `Merge` / `Clear` implementation ----
    let merge_impl = if options.merge {
        quote! {
            impl #p_impl_generics matchmaker_partial::Merge for #partial_ident #p_ty_generics #p_where_clause {
                fn merge(&mut self, other: Self) {
                    #(#merge_field_stmts)*
                }

                fn clear(&mut self) {
                    #(#clear_field_stmts)*
                }
            }
        }
    } else {
        quote! {}
    };

    let vis = &input.vis;
    let expanded = quote! {
        #input

        #(#deserializer_wrappers)*

        #(#final_attrs)*
        // The struct definition needs the full generics (not `#p_ty_generics`,
        // which would turn `const N: usize` into a plain type parameter).
        #vis struct #partial_ident #partial_generics {
            #(#partial_field_defs),*
        }

        impl #impl_generics matchmaker_partial::Apply for #struct_ident #ty_generics #where_clause {
            type Partial = #partial_ident #p_ty_generics;
            fn apply(&mut self, partial: Self::Partial) {
                #(#apply_field_stmts)*
            }
        }

        #merge_impl

        #path_setter_impl
    };

    TokenStream::from(expanded)
}

// ---------------------------------------------------------------------------
// Struct-level attribute parsing
// ---------------------------------------------------------------------------

/// Options parsed from the top-level `#[partial(...)]` attribute.
#[derive(Default)]
struct StructOptions {
    recurse: bool,
    unwrap: bool,
    path_setter: bool,
    merge: bool,
    /// Explicit `derive(...)` override; `None` means "inherit the struct's derives".
    manual_derives: Option<proc_macro2::TokenStream>,
    /// Explicit `attr(...)` overrides, replacing mirrored struct attributes.
    manual_attrs: Vec<proc_macro2::TokenStream>,
    /// Whether `attr(...)` was given (suppresses inheriting struct attributes).
    has_manual_attrs: bool,
    /// `attr(clear)` — do not mirror field attributes onto the partial.
    no_field_mirror: bool,
}

fn parse_struct_options(attr: TokenStream) -> SynResult<StructOptions> {
    let mut options = StructOptions::default();
    if attr.is_empty() {
        return Ok(options);
    }
    Parser::parse2(
        |input: syn::parse::ParseStream| {
            while !input.is_empty() {
                let path: Path = input.parse()?;
                if path.is_ident("recurse") {
                    options.recurse = true;
                } else if path.is_ident("unwrap") {
                    options.unwrap = true;
                } else if path.is_ident("path") {
                    options.path_setter = true;
                } else if path.is_ident("merge") {
                    options.merge = true;
                } else if path.is_ident("derive") {
                    if input.peek(syn::token::Paren) {
                        let content;
                        syn::parenthesized!(content in input);
                        let paths = content.parse_terminated(Path::parse, Token![,])?;
                        options.manual_derives = Some(quote! { #[derive(#paths)] });
                    } else {
                        // `derive` without parentheses: suppress derives entirely.
                        options.manual_derives = Some(quote! {});
                    }
                } else if path.is_ident("attr") {
                    options.has_manual_attrs = true;
                    if input.peek(syn::token::Paren) {
                        let content;
                        syn::parenthesized!(content in input);
                        let inner: Meta = content.parse()?;
                        if inner.path().is_ident("clear") {
                            options.no_field_mirror = true;
                        } else {
                            options.manual_attrs.push(quote! { #[#inner] });
                        }
                    }
                } else {
                    return Err(syn::Error::new(
                        path.span(),
                        format!("unknown partial attribute: {}", path.to_token_stream()),
                    ));
                }

                if input.peek(Token![,]) {
                    input.parse::<Token![,]>()?;
                }
            }
            Ok(())
        },
        attr.into(),
    )?;
    Ok(options)
}

/// Assembles the attributes of the generated `Partial*` struct: derives
/// (explicit override or inherited), an auto `#[derive(Default)]` unless one
/// is already present, and the remaining struct attributes.
fn build_partial_struct_attrs(
    input: &ItemStruct,
    options: &StructOptions,
) -> Vec<proc_macro2::TokenStream> {
    let mut final_attrs = Vec::new();
    let mut has_default = false;

    if let Some(manual) = &options.manual_derives {
        has_default = contains_default(manual);
        final_attrs.push(manual.clone());
    } else {
        for attr in &input.attrs {
            if attr.path().is_ident("derive") {
                let tokens = attr.to_token_stream();
                has_default |= contains_default(&tokens);
                final_attrs.push(tokens);
            }
        }
    }

    if !has_default {
        final_attrs.push(quote! { #[derive(Default)] });
    }

    if options.has_manual_attrs {
        final_attrs.extend(options.manual_attrs.iter().cloned());
    } else {
        for attr in &input.attrs {
            if !attr.path().is_ident("derive") {
                final_attrs.push(attr.to_token_stream());
            }
        }
    }

    final_attrs
}

/// Heuristic: does the token stream mention `Default`?
///
/// Kept as a string match rather than path parsing to preserve long-standing
/// behavior (e.g. `#[derive(MyDefaultThing)]` counts as providing `Default`).
fn contains_default(tokens: &proc_macro2::TokenStream) -> bool {
    tokens.to_string().contains("Default")
}

fn struct_generic_idents(input: &ItemStruct) -> HashSet<Ident> {
    let mut idents = HashSet::new();
    for param in &input.generics.params {
        match param {
            GenericParam::Type(t) => {
                idents.insert(t.ident.clone());
            }
            GenericParam::Const(c) => {
                idents.insert(c.ident.clone());
            }
            GenericParam::Lifetime(_) => {}
        }
    }
    idents
}

// ---------------------------------------------------------------------------
// Field-level attribute parsing
// ---------------------------------------------------------------------------

/// Struct-level context needed while processing each field.
struct FieldCtx<'a> {
    struct_ident: &'a Ident,
    struct_recurse: bool,
    struct_unwrap: bool,
    no_field_mirror: bool,
    /// Generic params of the original struct (a generated wrapper fn cannot
    /// reference them, so fields using them can't get a wrapper).
    generic_idents: &'a HashSet<Ident>,
}

/// Options parsed from the field-level `#[partial(...)]` attribute.
#[derive(Default)]
struct FieldOptions {
    skip: bool,
    unwrap: bool,
    recurse: bool,
    set: SetMode,
    /// `recurse = "Type"` override: `Some(None)` = explicit no-recurse,
    /// `Some(Some(tokens))` = explicit replacement type.
    recurse_override: Option<Option<proc_macro2::TokenStream>>,
}

/// What `set = "..."` does for a field's path-setter arm.
#[derive(Clone, Copy, Default, PartialEq)]
enum SetMode {
    /// Default path-based set behavior.
    #[default]
    Path,
    /// `set = "sequence"`: deserialize the full value as the collection.
    Sequence,
    /// `set = "recurse"`: keep descending into the collection element.
    Recurse,
}

/// Everything gathered while scanning a field's attributes.
#[derive(Default)]
struct FieldState {
    options: FieldOptions,
    /// Custom deserializer referenced by `#[serde(deserialize_with = ...)]` /
    /// `#[serde(with = ...)]`; used by the leaf path-setter logic.
    custom_deserializer: Option<Path>,
    /// Field name aliases for the path-setter (from `#[partial(alias)]` and
    /// `#[serde(alias)]`).
    aliases: Vec<String>,
    /// Set by `#[partial(flatten)]` / `#[serde(flatten)]`.
    is_flattened: bool,
    /// Wrapper fns generated for custom deserializers (promoted to the output
    /// only if the field survives the `skip` check).
    wrappers: Vec<proc_macro2::TokenStream>,
    /// Attributes mirrored onto the partial field.
    mirror_attrs: Vec<proc_macro2::TokenStream>,
    errors: Vec<syn::Error>,
}

/// Scans `field.attrs` in source order, parsing `#[partial]` and `#[serde]`
/// attributes and rebuilding the kept attribute list in place.
///
/// Note: attribute order matters — `should_recurse` (used when processing a
/// `#[serde]` attribute) reflects only the `#[partial]` attributes seen so far.
fn parse_field_attrs(field: &mut syn::Field, ctx: &FieldCtx) -> FieldState {
    let mut state = FieldState::default();
    state.options.unwrap = ctx.struct_unwrap;

    let field_ty = &field.ty;
    let is_opt = is_option(field_ty);
    let name_ident = field
        .ident
        .as_ref()
        .expect("Partial only supports structs with named fields")
        .unraw();
    // True when the field type references one of the struct's generic params
    // (a generated wrapper fn cannot reference them).
    let uses_struct_generics = {
        let mut idents = HashSet::new();
        find_idents_in_tokens(field_ty.to_token_stream(), &mut idents);
        ctx.generic_idents.iter().any(|g| idents.contains(g))
    };

    let mut kept = Vec::new();

    for attr in field.attrs.drain(..) {
        if attr.path().is_ident("partial") {
            // Control attribute: never kept on the original struct.
            if let Err(e) = parse_field_partial_attr(&attr, &mut state) {
                state.errors.push(e);
            }
            continue;
        }
        if attr.path().is_ident("serde") {
            if process_serde_attr(
                &attr,
                ctx,
                &mut state,
                field_ty,
                &name_ident,
                is_opt,
                uses_struct_generics,
            ) {
                kept.push(attr);
            }
            continue;
        }
        if !ctx.no_field_mirror {
            state.mirror_attrs.push(attr.to_token_stream());
        }
        kept.push(attr);
    }

    field.attrs = kept;
    state
}

fn parse_field_partial_attr(attr: &Attribute, state: &mut FieldState) -> SynResult<()> {
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("skip") {
            state.options.skip = true;
        } else if meta.path.is_ident("unwrap") {
            state.options.unwrap = true;
        } else if meta.path.is_ident("set") {
            let s: LitStr = meta.value()?.parse()?;
            state.options.set = match s.value().as_str() {
                "sequence" => SetMode::Sequence,
                "recurse" => SetMode::Recurse,
                other => {
                    return Err(meta.error(format!(
                        "unknown set mode: {other:?} (expected \"sequence\" or \"recurse\")"
                    )));
                }
            };
        } else if meta.path.is_ident("alias") {
            let s: LitStr = meta.value()?.parse()?;
            state.aliases.push(s.value());
        } else if meta.path.is_ident("flatten") {
            state.is_flattened = true;
        } else if meta.path.is_ident("recurse") {
            match meta.value() {
                Ok(value) => {
                    let s: LitStr = value.parse()?;
                    if s.value().is_empty() {
                        // `recurse = ""`: explicit no-recurse.
                        state.options.recurse_override = Some(None);
                    } else {
                        let ty: Type = s.parse().map_err(|e| {
                            syn::Error::new(s.span(), format!("invalid type in recurse: {e}"))
                        })?;
                        state.options.recurse_override = Some(Some(quote! { #ty }));
                    }
                }
                Err(_) => {
                    // Bare `recurse`: recurse using the default naming convention.
                    state.options.recurse = true;
                }
            }
        } else if meta.path.is_ident("no_recurse") {
            state.options.recurse_override = Some(None);
        } else if meta.path.is_ident("attr") {
            // Replaces everything mirrored so far.
            state.mirror_attrs.clear();
            if meta.input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in meta.input);
                while !content.is_empty() {
                    let inner_meta: Meta = content.parse()?;
                    state.mirror_attrs.push(quote! { #[#inner_meta] });
                    if content.peek(Token![,]) {
                        content.parse::<Token![,]>()?;
                    }
                }
            }
        } else {
            return Err(meta.error(format!(
                "unknown partial attribute: {}",
                meta.path.to_token_stream()
            )));
        }
        Ok(())
    })
}

/// Processes a `#[serde(...)]` attribute and returns whether it should stay on
/// the original struct (it always does — the attribute is the user's own).
///
/// Depending on the situation the partial gets:
/// - the attribute mirrored as-is (field type is unchanged by the partial),
/// - a rewritten `deserialize_with = "wrapper"` attribute pointing at a
///   generated wrapper fn that turns `T` into `Some(T)` (non-`Option` field
///   with a custom deserializer),
/// - nothing (custom deserializer cannot be mirrored, e.g. the field type uses
///   the struct's generics, or the field recurses / is unwrapped).
fn process_serde_attr(
    attr: &Attribute,
    ctx: &FieldCtx,
    state: &mut FieldState,
    field_ty: &Type,
    name_ident: &Ident,
    is_opt: bool,
    uses_struct_generics: bool,
) -> bool {
    let should_recurse = should_recurse_field(ctx, &state.options);
    // True when the partial mirrors the field with exactly the same type, so a
    // custom deserializer can be mirrored verbatim.
    let is_same_type = !should_recurse && (state.options.unwrap == !is_opt);
    // True when we can generate a small wrapper turning `T` into `Some(T)` for
    // the partial's `Option<T>` field.
    let can_wrap = !should_recurse
        && !is_opt
        && !state.options.unwrap
        && !ctx.no_field_mirror
        && !uses_struct_generics;

    let mut drop_attr = false;
    // (wrapper name, custom deserializer) — set when wrapping is needed.
    let mut wrapper: Option<(Ident, Path)> = None;
    let wrapper_name = || format_ident!("__mm_partial_deser_{}_{}", ctx.struct_ident, name_ident);

    if let Err(e) = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("deserialize_with") {
            let s: LitStr = meta.value()?.parse()?;
            let path: Path = s.parse().map_err(|e| {
                syn::Error::new(s.span(), format!("invalid path in deserialize_with: {e}"))
            })?;
            state.custom_deserializer = Some(path.clone());
            if !is_same_type {
                if can_wrap {
                    wrapper = Some((wrapper_name(), path));
                } else {
                    drop_attr = true;
                }
            }
        } else if meta.path.is_ident("with") {
            let s: LitStr = meta.value()?.parse()?;
            let mut path: Path = s
                .parse()
                .map_err(|e| syn::Error::new(s.span(), format!("invalid path in with: {e}")))?;
            path.segments.push(format_ident!("deserialize").into());
            state.custom_deserializer = Some(path.clone());
            if !is_same_type {
                if can_wrap {
                    wrapper = Some((wrapper_name(), path));
                } else {
                    drop_attr = true;
                }
            }
        } else if meta.path.is_ident("serialize_with") {
            // The partial's field type differs from the original's, so the
            // serializer doesn't apply there — keep it on the original only.
            // (Consume the value so `parse_nested_meta` accepts the meta.)
            meta.value()?.parse::<proc_macro2::TokenStream>()?;
            if !is_same_type {
                drop_attr = true;
            }
        } else if meta.path.is_ident("alias") {
            let s: LitStr = meta.value()?.parse()?;
            state.aliases.push(s.value());
        } else if meta.path.is_ident("flatten") {
            state.is_flattened = true;
        } else {
            // Unknown serde meta (e.g. `rename`, `default`): parse its value
            // (if any) so `parse_nested_meta` accepts it, then ignore — it
            // still applies to the partial via the mirrored attribute copy.
            if let Ok(value) = meta.value() {
                value.parse::<proc_macro2::TokenStream>()?;
            }
        }
        Ok(())
    }) {
        state.errors.push(e);
    }

    if drop_attr {
        // Custom deserializer can't be mirrored (e.g. the field type uses the
        // struct's generics, or the field recurses / is unwrapped). Keep the
        // attribute on the original struct; don't mirror it.
        return true;
    }

    if let Some((name, func)) = wrapper {
        // The partial field stays `Option<T>` but parses through the original
        // custom deserializer via a generated wrapper fn.
        state.wrappers.push(quote! {
            #[doc(hidden)]
            // The wrapper name embeds the struct & field idents
            // (e.g. `__mm_partial_deser_StartConfig_shell`), which
            // is not snake_case when the struct is CamelCase.
            #[allow(non_snake_case)]
            fn #name<'de, D>(__d: D) -> Result<Option<#field_ty>, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                Ok(Some(#func(__d)?))
            }
        });
        state
            .mirror_attrs
            .push(rewrite_serde_attr_to_wrapper(attr, &name));
        return true;
    }

    // Same-type field: mirror the attribute verbatim.
    if !ctx.no_field_mirror {
        state.mirror_attrs.push(attr.to_token_stream());
    }
    true
}

fn should_recurse_field(ctx: &FieldCtx, options: &FieldOptions) -> bool {
    (ctx.struct_recurse || options.recurse || options.recurse_override.is_some())
        && !matches!(options.recurse_override, Some(None))
}

/// Rewrites the last path segment of a type with a `Partial` prefix
/// (`Foo` → `PartialFoo`, `a::Foo` → `a::PartialFoo`); non-path types pass
/// through unchanged.
fn partial_type_of(ty: &Type) -> proc_macro2::TokenStream {
    if let Type::Path(tp) = ty {
        let mut p = tp.path.clone();
        if let Some(seg) = p.segments.last_mut() {
            seg.ident = format_ident!("Partial{}", seg.ident);
            quote! { #p }
        } else {
            ty.to_token_stream()
        }
    } else {
        ty.to_token_stream()
    }
}

// ---------------------------------------------------------------------------
// Per-field codegen
// ---------------------------------------------------------------------------

/// Codegen output for one field: the partial field type plus the statements
/// and arms for the `Apply`, `Merge`/`Clear` and `Set` impls.
struct FieldCodegen {
    /// Partial field type (also used to track generic usage).
    ty: proc_macro2::TokenStream,
    apply: proc_macro2::TokenStream,
    merge: proc_macro2::TokenStream,
    clear: proc_macro2::TokenStream,
    /// Path-setter arm; `None` when the field is a flattened recursive field.
    set_arm: Option<proc_macro2::TokenStream>,
    /// Fallback set target for flattened recursive fields.
    flattened_target: Option<proc_macro2::TokenStream>,
}

fn codegen_collection_field(
    state: &FieldState,
    raw_ident: &Ident,
    _field_ty: &Type,
    is_opt: bool,
    should_recurse: bool,
    coll: &CollectionInfo<'_>,
) -> SynResult<FieldCodegen> {
    let kind = coll.kind;
    let key_ty = coll.key_ty;
    let element_ty = coll.element_ty;

    let mut is_recursive_field = false;
    let partial_element_ty = if should_recurse {
        is_recursive_field = true;
        if let Some(Some(overridden)) = &state.options.recurse_override {
            overridden.clone()
        } else {
            partial_type_of(element_ty)
        }
    } else {
        quote! { #element_ty }
    };

    let coll_ident = match kind {
        CollectionKind::Vec => quote! { Vec },
        CollectionKind::HashSet => quote! { HashSet },
        CollectionKind::BTreeSet => quote! { BTreeSet },
        CollectionKind::HashMap => quote! { HashMap },
        CollectionKind::BTreeMap => quote! { BTreeMap },
    };

    let partial_coll_ty = if let Some(key) = key_ty {
        quote! { #coll_ident<#key, #partial_element_ty> }
    } else {
        quote! { #coll_ident<#partial_element_ty> }
    };

    let ty = if state.options.unwrap {
        partial_coll_ty.clone()
    } else {
        quote! { Option<#partial_coll_ty> }
    };

    // --- Apply logic ---
    let target_expr = if is_opt {
        quote! { self.#raw_ident.get_or_insert_with(Default::default) }
    } else {
        quote! { self.#raw_ident }
    };

    let apply = if is_recursive_field {
        let element_apply = match kind {
            CollectionKind::Vec | CollectionKind::HashSet | CollectionKind::BTreeSet => {
                let push_method = if kind == CollectionKind::Vec {
                    quote! { push }
                } else {
                    quote! { insert }
                };
                if !state.options.unwrap {
                    if kind == CollectionKind::Vec {
                        quote! {
                            let mut p_it = p.into_iter();
                            for target in #target_expr.iter_mut() {
                                if let Some(p_item) = p_it.next() {
                                    matchmaker_partial::Apply::apply(target, p_item);
                                } else {
                                    break;
                                }
                            }
                            for p_item in p_it {
                                let mut t = <#element_ty as Default>::default();
                                matchmaker_partial::Apply::apply(&mut t, p_item);
                                #target_expr.push(t);
                            }
                        }
                    } else {
                        quote! {
                            for p_item in p {
                                let mut t = <#element_ty as Default>::default();
                                matchmaker_partial::Apply::apply(&mut t, p_item);
                                #target_expr.insert(t);
                            }
                        }
                    }
                } else {
                    quote! {
                        for p_item in partial.#raw_ident {
                            let mut t = <#element_ty as Default>::default();
                            matchmaker_partial::Apply::apply(&mut t, p_item);
                            #target_expr.#push_method(t);
                        }
                    }
                }
            }
            CollectionKind::HashMap | CollectionKind::BTreeMap => {
                let src = if state.options.unwrap {
                    quote! { partial.#raw_ident }
                } else {
                    quote! { p }
                };
                quote! {
                    for (k, p_v) in #src {
                        if let Some(v) = #target_expr.get_mut(&k) {
                            matchmaker_partial::Apply::apply(v, p_v);
                        } else {
                            let mut v = <#element_ty as Default>::default();
                            matchmaker_partial::Apply::apply(&mut v, p_v);
                            #target_expr.insert(k, v);
                        }
                    }
                }
            }
        };
        if state.options.unwrap {
            element_apply
        } else {
            quote! { if let Some(p) = partial.#raw_ident { #element_apply } }
        }
    } else if !state.options.unwrap {
        let val = if is_opt {
            quote! { Some(p) }
        } else {
            quote! { p }
        };
        quote! { if let Some(p) = partial.#raw_ident { self.#raw_ident = #val; } }
    } else if matches!(kind, CollectionKind::HashMap | CollectionKind::BTreeMap) {
        quote! {
            for (k, v) in partial.#raw_ident {
                #target_expr.insert(k, v);
            }
        }
    } else {
        quote! { #target_expr.extend(partial.#raw_ident.into_iter()); }
    };

    // --- Merge / Clear logic ---
    let (merge, clear) = if !state.options.unwrap {
        (
            quote! {
                if let Some(other_coll) = other.#raw_ident {
                    self.#raw_ident.get_or_insert_with(Default::default).extend(other_coll.into_iter());
                }
            },
            quote! { self.#raw_ident = None; },
        )
    } else {
        (
            quote! { self.#raw_ident.extend(other.#raw_ident.into_iter()); },
            quote! { self.#raw_ident.clear(); },
        )
    };

    // --- Set logic ---
    let field_name_str = {
        let s = raw_ident.to_string();
        s.strip_prefix("r#").unwrap_or(&s).to_string()
    };
    let is_sequence = state.options.set == SetMode::Sequence;
    let is_set_recurse = state.options.set == SetMode::Recurse;

    let set_logic = if is_sequence {
        let assignment = if !state.options.unwrap {
            quote! { self.#raw_ident = Some(deserialized); }
        } else {
            quote! { self.#raw_ident.extend(deserialized); }
        };
        quote! {
            let deserialized: #partial_coll_ty = matchmaker_partial::deserialize(val)?;
            #assignment
        }
    } else {
        let target = if !state.options.unwrap {
            quote! { self.#raw_ident.get_or_insert_with(Default::default) }
        } else {
            quote! { self.#raw_ident }
        };

        let set_full_coll_logic = if !state.options.unwrap {
            quote! { self.#raw_ident = Some(new_map); }
        } else {
            quote! { #target.extend(new_map.into_iter()); }
        };

        let p_element_ty = partial_type_of(element_ty);

        if let Some(key_ty) = key_ty {
            let val_ty = if should_recurse {
                quote! { #partial_element_ty }
            } else {
                quote! { #element_ty }
            };

            let descent_logic = if should_recurse || is_set_recurse {
                let set_item_logic = if should_recurse {
                    quote! { matchmaker_partial::Set::set(item, rest, val)?; }
                } else {
                    quote! {
                        let mut p_item = #p_element_ty::default();
                        matchmaker_partial::Set::set(&mut p_item, rest, val)?;
                        *item = matchmaker_partial::from(p_item);
                    }
                };

                quote! {
                    if rest.is_empty() {
                        let mut combined = vec![key_str.clone()];
                        combined.extend_from_slice(val);
                        let (key, value): (#key_ty, #val_ty) = matchmaker_partial::deserialize(&combined)?;
                        let _ = #target.insert(key, value);
                    } else {
                        let key: #key_ty = matchmaker_partial::deserialize(&[key_str.clone()])?;
                        let item = #target.entry(key).or_insert_with(Default::default);
                        #set_item_logic
                    }
                }
            } else {
                quote! {
                    if rest.is_empty() {
                        let key: #key_ty = matchmaker_partial::deserialize(&[key_str.clone()])?;
                        let value: #val_ty = matchmaker_partial::deserialize(&val)?;
                        let _ = #target.insert(key, value);
                    } else {
                        return Err(matchmaker_partial::PartialSetError::ExtraPaths(rest.to_vec()));
                    }
                }
            };

            quote! {
                if let Some((key_str, rest)) = tail.split_first() {
                    #descent_logic
                } else {
                    let new_map: #partial_coll_ty = matchmaker_partial::deserialize(val)?;
                    #set_full_coll_logic
                }
            }
        } else {
            let push_method = match kind {
                CollectionKind::Vec => quote! { push },
                _ => quote! { insert },
            };
            let item_ty = if should_recurse {
                quote! { #partial_element_ty }
            } else {
                quote! { #element_ty }
            };
            if is_set_recurse {
                if should_recurse {
                    quote! {
                        let mut item = #item_ty::default();
                        if tail.is_empty() {
                            item = matchmaker_partial::deserialize(val)?;
                        } else {
                            matchmaker_partial::Set::set(&mut item, tail, val)?;
                        }
                        #target.#push_method(item);
                    }
                } else {
                    quote! {
                        if tail.is_empty() {
                            let item: #item_ty = matchmaker_partial::deserialize(val)?;
                            #target.#push_method(item);
                        } else {
                            let mut p_item = #p_element_ty::default();
                            matchmaker_partial::Set::set(&mut p_item, tail, val)?;
                            let item: #item_ty = matchmaker_partial::from(p_item);
                            #target.#push_method(item);
                        }
                    }
                }
            } else {
                quote! {
                    if let Some((_, _)) = tail.split_first() {
                        return Err(matchmaker_partial::PartialSetError::ExtraPaths(tail.to_vec()));
                    }
                    let item: #item_ty = matchmaker_partial::deserialize(val)?;
                    #target.#push_method(item);
                }
            }
        }
    };

    Ok(FieldCodegen {
        ty,
        apply,
        merge,
        clear,
        set_arm: Some({
            let aliases = &state.aliases;
            quote! {
                #field_name_str #(| #aliases)* => {
                    #set_logic
                    Ok(())
                }
            }
        }),
        flattened_target: None,
    })
}

fn codegen_leaf_field(
    state: &FieldState,
    raw_ident: &Ident,
    field_ty: &Type,
    inner_ty: &Type,
    is_opt: bool,
    should_recurse: bool,
) -> SynResult<FieldCodegen> {
    let mut is_recursive_field = false;
    let ty = if should_recurse {
        is_recursive_field = true;
        // The partial mirrors the recursive field as the `Partial*` type,
        // wrapped in `Option` unless the field is unwrapped or already `Option`.
        let p_ty = if let Some(Some(overridden)) = &state.options.recurse_override {
            overridden.clone()
        } else {
            partial_type_of(inner_ty)
        };

        if !state.options.unwrap && is_opt {
            quote! { Option<#p_ty> }
        } else {
            p_ty
        }
    } else if state.options.unwrap {
        quote! { #inner_ty }
    } else if is_opt {
        quote! { #field_ty }
    } else {
        quote! { Option<#field_ty> }
    };

    let field_name_str = {
        let s = raw_ident.to_string();
        s.strip_prefix("r#").unwrap_or(&s).to_string()
    };

    let (apply, merge, clear) = if is_recursive_field {
        if !state.options.unwrap && is_opt {
            (
                quote! {
                    if let Some(p) = partial.#raw_ident {
                        if let Some(ref mut v) = self.#raw_ident {
                            matchmaker_partial::Apply::apply(v, p);
                        } else {
                            self.#raw_ident = Some(matchmaker_partial::from(p));
                        }
                    }
                },
                quote! {
                    match (&mut self.#raw_ident, other.#raw_ident) {
                        (Some(s), Some(o)) => matchmaker_partial::Merge::merge(s, o),
                        (t @ None, Some(o)) => *t = Some(o),
                        _ => {}
                    }
                },
                quote! { self.#raw_ident = None; },
            )
        } else {
            let apply = if state.options.unwrap && is_opt {
                quote! {
                    if let Some(ref mut v) = self.#raw_ident {
                        matchmaker_partial::Apply::apply(v, partial.#raw_ident);
                    } else {
                        self.#raw_ident = Some(matchmaker_partial::from(partial.#raw_ident));
                    }
                }
            } else {
                quote! { matchmaker_partial::Apply::apply(&mut self.#raw_ident, partial.#raw_ident); }
            };
            (
                apply,
                quote! { matchmaker_partial::Merge::merge(&mut self.#raw_ident, other.#raw_ident); },
                quote! { matchmaker_partial::Merge::clear(&mut self.#raw_ident); },
            )
        }
    } else {
        let apply = if state.options.unwrap {
            if is_opt {
                quote! { self.#raw_ident = Some(partial.#raw_ident); }
            } else {
                quote! { self.#raw_ident = partial.#raw_ident; }
            }
        } else if !is_opt {
            quote! { if let Some(v) = partial.#raw_ident { self.#raw_ident = v; } }
        } else {
            quote! { if let Some(v) = partial.#raw_ident { self.#raw_ident = Some(v); } }
        };
        (
            apply,
            quote! { if other.#raw_ident.is_some() { self.#raw_ident = other.#raw_ident; } },
            quote! { self.#raw_ident = None; },
        )
    };

    let (set_arm, flattened_target) = if is_recursive_field {
        let set_target = if is_opt {
            quote! { self.#raw_ident.get_or_insert_with(Default::default) }
        } else {
            quote! { &mut self.#raw_ident }
        };

        if state.is_flattened {
            (None, Some(set_target))
        } else {
            let aliases = &state.aliases;
            (
                Some(quote! {
                    #field_name_str #(| #aliases)* => {
                        if tail.is_empty() {
                            return Err(matchmaker_partial::PartialSetError::EarlyEnd(head.clone()));
                        }
                        matchmaker_partial::Set::set(#set_target, tail, val)
                    }
                }),
                None,
            )
        }
    } else {
        // Custom deserializer: invoke it directly (it expects a Deserializer).
        // Otherwise the generic deserialize helper returns the inner type T.
        let set_logic = if let Some(custom_func) = &state.custom_deserializer {
            // The custom fn returns `T` (or `Option<T>` for `Option` fields),
            // so it assigns directly unless the partial field is `Option<T>`
            // while the fn returns `T`.
            let assignment = if state.options.unwrap || is_opt {
                quote! { self.#raw_ident = result; }
            } else {
                quote! { self.#raw_ident = Some(result); }
            };

            quote! {
                let mut deserializer = matchmaker_partial::SimpleDeserializer::from_slice(val);
                let result = #custom_func(&mut deserializer)?;
                #assignment
            }
        } else {
            let inner_ty = extract_inner_type_from_option(field_ty);
            let assignment = if state.options.unwrap {
                quote! { self.#raw_ident = deserialized; }
            } else {
                quote! { self.#raw_ident = Some(deserialized); }
            };
            quote! {
                let deserialized = matchmaker_partial::deserialize::<#inner_ty>(val)?;
                #assignment
            }
        };

        (
            Some({
                let aliases = &state.aliases;
                quote! {
                    #field_name_str #(| #aliases)* => {
                        if !tail.is_empty() {
                            return Err(matchmaker_partial::PartialSetError::ExtraPaths(tail.to_vec()));
                        }
                        #set_logic
                        Ok(())
                    }
                }
            }),
            None,
        )
    };

    Ok(FieldCodegen {
        ty,
        apply,
        merge,
        clear,
        set_arm,
        flattened_target,
    })
}

// ---------------------------------------------------------------------------
// Type helpers
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Clone, Copy)]
enum CollectionKind {
    Vec,
    HashSet,
    BTreeSet,
    HashMap,
    BTreeMap,
}

/// A recognized collection type and its type arguments.
struct CollectionInfo<'a> {
    kind: CollectionKind,
    /// `Some` for two-argument collections (maps; also the legacy pass-through
    /// for e.g. `Vec<T, A>`).
    key_ty: Option<&'a Type>,
    element_ty: &'a Type,
}

fn get_collection_info(ty: &Type) -> SynResult<Option<CollectionInfo<'_>>> {
    if let Type::Path(tp) = ty
        && let Some(last_seg) = tp.path.segments.last()
    {
        let kind = if last_seg.ident == "Vec" {
            CollectionKind::Vec
        } else if last_seg.ident == "HashSet" {
            CollectionKind::HashSet
        } else if last_seg.ident == "BTreeSet" {
            CollectionKind::BTreeSet
        } else if last_seg.ident == "HashMap" {
            CollectionKind::HashMap
        } else if last_seg.ident == "BTreeMap" {
            CollectionKind::BTreeMap
        } else {
            return Ok(None);
        };

        let mut inner_types = Vec::new();
        if let PathArguments::AngleBracketed(args) = &last_seg.arguments {
            for arg in &args.args {
                if let GenericArgument::Type(inner_ty) = arg {
                    inner_types.push(inner_ty);
                }
            }
        }

        let is_map = matches!(kind, CollectionKind::HashMap | CollectionKind::BTreeMap);
        let (key_ty, element_ty) = match (is_map, inner_types.as_slice()) {
            (true, [key, value]) => (Some(*key), *value),
            (false, [element]) => (None, *element),
            // Legacy pass-through for two-argument non-map collections
            // (e.g. `Vec<T, A>`): keep the original type tokens.
            (false, [key, element]) => (Some(*key), *element),
            (_, []) => {
                return Err(syn::Error::new(
                    ty.span(),
                    "collection type is missing its type arguments",
                ));
            }
            (_, _) => {
                return Err(syn::Error::new(
                    ty.span(),
                    format!("unsupported number of type arguments for {kind:?}"),
                ));
            }
        };

        Ok(Some(CollectionInfo {
            kind,
            key_ty,
            element_ty,
        }))
    } else {
        Ok(None)
    }
}

fn is_option(ty: &Type) -> bool {
    if let Type::Path(tp) = ty {
        tp.path.segments.last().is_some_and(|s| s.ident == "Option")
    } else {
        false
    }
}

/// Helper to get 'T' out of 'Option<T>' or return 'T' if it's not an Option.
fn extract_inner_type_from_option(ty: &Type) -> &Type {
    if let Type::Path(tp) = ty
        && let Some(last_seg) = tp.path.segments.last()
        && last_seg.ident == "Option"
        && let PathArguments::AngleBracketed(args) = &last_seg.arguments
        && let Some(GenericArgument::Type(inner)) = args.args.first()
    {
        return inner;
    }
    ty
}

/// Rewrites a `#[serde(...)]` attribute so that `deserialize_with = "path"`
/// (or `with = "path"`) points at `wrapper` instead, keeping every other meta
/// (e.g. `alias`) untouched. `with` is rewritten to `deserialize_with`; the
/// partial only needs the deserialization half.
fn rewrite_serde_attr_to_wrapper(attr: &Attribute, wrapper: &Ident) -> proc_macro2::TokenStream {
    let meta = match &attr.meta {
        Meta::List(list) => list,
        _ => return attr.to_token_stream(),
    };
    let tokens: Vec<proc_macro2::TokenTree> = meta.tokens.clone().into_iter().collect();
    let mut out = proc_macro2::TokenStream::new();
    let mut i = 0;
    while i < tokens.len() {
        let is_key = matches!(&tokens[i], proc_macro2::TokenTree::Ident(id)
            if id == "deserialize_with" || id == "with");
        let is_eq = matches!(tokens.get(i + 1), Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == '=');
        let is_lit = matches!(tokens.get(i + 2), Some(proc_macro2::TokenTree::Literal(_)));
        if is_key && is_eq && is_lit {
            let wrapper_str = syn::LitStr::new(&wrapper.to_string(), wrapper.span());
            out.extend(quote! { deserialize_with = #wrapper_str });
            i += 3;
        } else {
            out.extend(std::iter::once(tokens[i].clone()));
            i += 1;
        }
    }
    quote! { #[serde(#out)] }
}

fn find_idents_in_tokens(tokens: proc_macro2::TokenStream, set: &mut HashSet<Ident>) {
    for token in tokens {
        match token {
            proc_macro2::TokenTree::Ident(id) => {
                set.insert(id);
            }
            proc_macro2::TokenTree::Group(g) => find_idents_in_tokens(g.stream(), set),
            _ => {}
        }
    }
}

/// Combines a list of errors into one (keeping all of them), if any.
fn drain_errors(errors: &mut Vec<syn::Error>) -> Option<syn::Error> {
    let mut iter = errors.drain(..);
    let mut combined = iter.next()?;
    for e in iter {
        combined.combine(e);
    }
    Some(combined)
}

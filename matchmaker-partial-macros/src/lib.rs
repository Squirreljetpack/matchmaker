use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use std::collections::HashSet;
use syn::{
    Fields, GenericArgument, ItemStruct, LitStr, Meta, Path, PathArguments, Token, Type,
    parse::Parse, parse_macro_input, spanned::Spanned,
};

#[proc_macro_attribute]
pub fn partial(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let partial_name = format_ident!("Partial{}", name);

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let vis = &input.vis;

    let mut struct_recurse = false;
    let mut generate_path_setter = false;
    let mut enable_merge = false; // Additive change: Gate for merge/clear
    let mut manual_derives: Option<proc_macro2::TokenStream> = None;
    let mut manual_attrs: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut has_manual_attrs = false;

    // --- 1. Parse macro arguments (Top level: #[partial(path, recurse, derive, attr)]) ---
    if !attr.is_empty() {
        let parser = syn::parse::Parser::parse2(
            |input: syn::parse::ParseStream| {
                while !input.is_empty() {
                    let path: Path = input.parse()?;
                    if path.is_ident("recurse") {
                        struct_recurse = true;
                    } else if path.is_ident("path") {
                        generate_path_setter = true;
                    } else if path.is_ident("merge") {
                        enable_merge = true; // Mark as merge-enabled
                    } else if path.is_ident("derive") {
                        // Check if derive has parentheses
                        if input.peek(syn::token::Paren) {
                            let content;
                            syn::parenthesized!(content in input);
                            let paths = content.parse_terminated(Path::parse, Token![,])?;
                            manual_derives = Some(quote! { #[derive(#paths)] });
                        } else {
                            // derive without parentheses -> just mark as manual (empty)
                            manual_derives = Some(quote! {});
                        }
                    } else if path.is_ident("attr") {
                        has_manual_attrs = true;
                        if input.peek(syn::token::Paren) {
                            let content;
                            syn::parenthesized!(content in input);
                            let inner: Meta = content.parse()?;
                            manual_attrs.push(quote! { #[#inner] });
                        }
                    } else {
                        // Error on unknown attributes
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
        );

        if let Err(e) = parser {
            return e.to_compile_error().into();
        }
    }

    // --- 2. Remove any #[partial] attributes from the struct & check for 'path' ---
    let mut attr_errors = Vec::new();
    input.attrs.retain(|attr| {
        if attr.path().is_ident("partial") {
            let res = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("recurse") {
                    struct_recurse = true;
                } else if meta.path.is_ident("path") {
                    generate_path_setter = true;
                } else if meta.path.is_ident("merge") {
                    enable_merge = true; // Mark as merge-enabled from struct attribute
                } else if meta.path.is_ident("derive") {
                    if meta.input.peek(syn::token::Paren) {
                        let content;
                        syn::parenthesized!(content in meta.input);
                        let paths = content.parse_terminated(Path::parse, Token![,]).unwrap();
                        manual_derives = Some(quote! { #[derive(#paths)] });
                    }
                } else if meta.path.is_ident("attr") {
                    has_manual_attrs = true;
                    if meta.input.peek(syn::token::Paren) {
                        let content;
                        syn::parenthesized!(content in meta.input);
                        let inner: Meta = content.parse().unwrap();
                        manual_attrs.push(quote! { #[#inner] });
                    }
                } else {
                    return Err(meta.error(format!(
                        "unknown partial attribute: {}",
                        meta.path.to_token_stream()
                    )));
                }
                Ok(())
            });

            if let Err(e) = res {
                attr_errors.push(e);
            }
            false
        } else {
            true
        }
    });

    if let Some(err) = attr_errors.first() {
        return err.to_compile_error().into();
    }

    // --- 3. Build final struct attributes ---
    let mut final_attrs = Vec::new();
    let mut has_default = false;

    if let Some(manual) = manual_derives {
        let manual_str = manual.to_token_stream().to_string();
        if manual_str.contains("Default") {
            has_default = true;
        }
        final_attrs.push(manual);
    } else {
        for attr in &input.attrs {
            if attr.path().is_ident("derive") {
                let tokens = attr.to_token_stream();
                if tokens.to_string().contains("Default") {
                    has_default = true;
                }
                final_attrs.push(tokens);
            }
        }
    }

    if !has_default {
        final_attrs.push(quote! { #[derive(Default)] });
    }

    if has_manual_attrs {
        final_attrs.extend(manual_attrs);
    } else {
        for attr in &input.attrs {
            if !attr.path().is_ident("derive") {
                final_attrs.push(attr.to_token_stream());
            }
        }
    }

    // --- 4. Process fields ---
    let fields = match &mut input.fields {
        Fields::Named(fields) => &mut fields.named,
        _ => panic!("Partial only supports structs with named fields"),
    };

    let mut partial_field_defs = Vec::new();
    let mut apply_field_stmts = Vec::new();
    let mut merge_field_stmts = Vec::new();
    let mut clear_field_stmts = Vec::new();
    let mut set_field_arms = Vec::new();
    let mut used_idents = HashSet::new();

    for field in fields.iter_mut() {
        let field_name = &field.ident;
        let field_vis = &field.vis;
        let field_ty = &field.ty;

        let mut skip_field = false;
        let mut recurse_override: Option<Option<proc_macro2::TokenStream>> = None;
        let mut field_attrs_for_mirror = Vec::new();
        let mut field_errors = Vec::new();
        // 4 Check for Serde custom deserialization attributes
        let mut custom_deserializer: Option<Path> = None;

        field.attrs.retain(|attr| {
            // --- 4a. Handle #[partial] attributes ---
            if attr.path().is_ident("partial") {
                let res = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("skip") {
                        skip_field = true;
                    } else if meta.path.is_ident("recurse") {
                        if let Ok(value) = meta.value() {
                            let s: LitStr = value.parse().unwrap();
                            if s.value().is_empty() {
                                recurse_override = Some(None);
                            } else {
                                let ty: Type = s.parse().unwrap();
                                recurse_override = Some(Some(quote! { #ty }));
                            }
                        } else if let Type::Path(ty_path) = field_ty {
                            let mut p_path = ty_path.path.clone();
                            if let Some(seg) = p_path.segments.last_mut() {
                                seg.ident = format_ident!("Partial{}", seg.ident);
                                recurse_override = Some(Some(quote! { #p_path }));
                            }
                        }
                    } else if meta.path.is_ident("attr") {
                        field_attrs_for_mirror.clear();
                        if meta.input.peek(syn::token::Paren) {
                            let content;
                            syn::parenthesized!(content in meta.input);
                            while !content.is_empty() {
                                let inner_meta: Meta = content.parse()?;
                                field_attrs_for_mirror.push(quote! { #[#inner_meta] });
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
                });

                if let Err(e) = res {
                    field_errors.push(e);
                }
                return false; // Always drop #[partial]
            }

            // --- 4b. Handle #[serde] attributes ---
            if attr.path().is_ident("serde") {
                let mut has_deserializer = false;
                let _ = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("deserialize_with") {
                        if let Ok(value) = meta.value() {
                            if let Ok(s) = value.parse::<LitStr>() {
                                custom_deserializer = s.parse::<Path>().ok();
                                has_deserializer = true;
                            }
                        }
                    } else if meta.path.is_ident("with") {
                        if let Ok(value) = meta.value() {
                            if let Ok(s) = value.parse::<LitStr>() {
                                if let Ok(mut p) = s.parse::<Path>() {
                                    p.segments.push(format_ident!("deserialize").into());
                                    custom_deserializer = Some(p);
                                    has_deserializer = true;
                                }
                            }
                        }
                    }
                    Ok(())
                });

                if has_deserializer {
                    return false; // Drop the #[serde] attribute
                }
            }

            // Keep the attribute and mirror it if it's not a #[partial]
            field_attrs_for_mirror.push(attr.to_token_stream());
            true
        });

        if let Some(err) = field_errors.first() {
            return err.to_compile_error().into();
        }

        if skip_field {
            continue;
        }

        let is_opt = is_option(field_ty);
        let final_recurse_ty = match recurse_override {
            Some(target) => target,
            None if struct_recurse => {
                if let Type::Path(ty_path) = field_ty {
                    let mut p_path = ty_path.path.clone();
                    if let Some(seg) = p_path.segments.last_mut() {
                        seg.ident = format_ident!("Partial{}", seg.ident);
                        Some(quote! { #p_path })
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        };

        let current_field_ty = if let Some(target_ty) = final_recurse_ty {
            // Detect common collection types for recursive logic
            if is_collection(field_ty) {
                // Emit original type (no Option wrap) and use .extend()
                apply_field_stmts
                    .push(quote! { self.#field_name.extend(partial.#field_name.into_iter()); });
                merge_field_stmts
                    .push(quote! { self.#field_name.extend(other.#field_name.into_iter()); });
                clear_field_stmts.push(quote! { self.#field_name.clear(); });
                quote! { #field_ty }
            } else {
                // Recursive field handling
                apply_field_stmts.push(quote! { self.#field_name.apply(partial.#field_name); });
                merge_field_stmts.push(quote! { self.#field_name.merge(other.#field_name); });
                clear_field_stmts.push(quote! { self.#field_name.clear(); });

                if let Some(field_ident) = &field.ident {
                    let field_name_str = field_ident.to_string();
                    let field_name_str =
                        field_name_str.strip_prefix("r#").unwrap_or(&field_name_str);

                    set_field_arms.push(quote! {
                        #field_name_str => {
                            if tail.is_empty() {
                                return Err(matchmaker_partial::PartialSetError::EarlyEnd(head.clone()));
                            }
                            self.#field_ident.set(tail, val)
                        }
                    });
                }
                target_ty
            }
        } else {
            // Leaf field handling
            let actual_ty = if is_opt {
                quote! { #field_ty }
            } else {
                quote! { Option<#field_ty> }
            };

            if is_opt {
                apply_field_stmts.push(
                    quote! { if let Some(v) = partial.#field_name { self.#field_name = Some(v); } },
                );
            } else {
                apply_field_stmts.push(
                    quote! { if let Some(v) = partial.#field_name { self.#field_name = v; } },
                );
            }
            merge_field_stmts.push(
                quote! { if other.#field_name.is_some() { self.#field_name = other.#field_name; } },
            );
            clear_field_stmts.push(quote! { self.#field_name = None; });

            if let Some(field_ident) = &field.ident {
                let field_name_str = field_ident.to_string();
                let field_name_str = field_name_str.strip_prefix("r#").unwrap_or(&field_name_str);

                // Determine deserialization logic
                let set_logic = if let Some(custom_func) = custom_deserializer {
                    // Logic: custom_func expects a Deserializer.
                    // If the field is Option<T>, deserialize_with returns Option<T>, so we assign directly.
                    // If the field is T, deserialize_with returns T, so we must wrap in Some().
                    let assignment = if is_opt {
                        quote! { self.#field_name = result; }
                    } else {
                        quote! { self.#field_name = Some(result); }
                    };

                    quote! {
                        let deserializer = matchmaker_partial::SimpleDeserializer::from_slice(val);
                        let result = #custom_func(deserializer)?;
                        #assignment
                    }
                } else {
                    // Logic: generic deserialize helper returns the inner type T.
                    // We always assign Some(T).
                    let inner_ty = extract_inner_type(field_ty);
                    quote! {
                        let deserialized = matchmaker_partial::deserialize::<#inner_ty>(val)?;
                        self.#field_name = Some(deserialized);
                    }
                };

                set_field_arms.push(quote! {
                    #field_name_str => {
                        if !tail.is_empty() {
                            return Err(matchmaker_partial::PartialSetError::ExtraPaths(tail.to_vec()));
                        }
                        #set_logic
                        Ok(())
                    }
                });
            }

            actual_ty
        };

        find_idents_in_tokens(current_field_ty.clone(), &mut used_idents);
        partial_field_defs
            .push(quote! { #(#field_attrs_for_mirror)* #field_vis #field_name: #current_field_ty });
    }

    // --- 5. Nuanced Generics Handling ---
    let mut partial_generics = input.generics.clone();
    partial_generics.params = partial_generics
        .params
        .into_iter()
        .filter(|param| match param {
            syn::GenericParam::Type(t) => used_idents.contains(&t.ident),
            syn::GenericParam::Lifetime(l) => used_idents.contains(&l.lifetime.ident),
            syn::GenericParam::Const(c) => used_idents.contains(&c.ident),
        })
        .collect();

    let (p_impl_generics, p_ty_generics, p_where_clause) = partial_generics.split_for_impl();

    // --- 6. Optional Path Setter Implementation ---
    let path_setter_impl = if generate_path_setter {
        quote! {
            impl #p_impl_generics #partial_name #p_ty_generics #p_where_clause {
                pub fn set(&mut self, path: &[String], val: &[String]) -> Result<(), matchmaker_partial::PartialSetError> {
                    let (head, tail) = path.split_first().ok_or_else(|| {
                        matchmaker_partial::PartialSetError::EarlyEnd("root".to_string())
                    })?;

                    match head.as_str() {
                        #(#set_field_arms)*
                        _ => Err(matchmaker_partial::PartialSetError::Missing(head.clone())),
                    }
                }
            }
        }
    } else {
        quote! {}
    };

    // --- 7. Conditional Merge/Clear auto-impl ---
    let merge_impl = if enable_merge {
        quote! {
            impl #p_impl_generics #partial_name #p_ty_generics #p_where_clause {
                pub fn merge(&mut self, other: Self) {
                    #(#merge_field_stmts)*
                }

                pub fn clear(&mut self) {
                    #(#clear_field_stmts)*
                }
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #input

        #(#final_attrs)*
        #vis struct #partial_name #p_ty_generics #p_where_clause {
            #(#partial_field_defs),*
        }

        impl #impl_generics #name #ty_generics #where_clause {
            pub fn apply(&mut self, partial: #partial_name #p_ty_generics) {
                #(#apply_field_stmts)*
            }
        }

        #merge_impl

        #path_setter_impl
    };

    TokenStream::from(expanded)
}

fn is_option(ty: &Type) -> bool {
    if let Type::Path(tp) = ty {
        tp.path
            .segments
            .last()
            .map_or(false, |s| s.ident == "Option")
    } else {
        false
    }
}

// Additive change: Helper to identify growable collection types
fn is_collection(ty: &Type) -> bool {
    if let Type::Path(tp) = ty {
        tp.path.segments.last().map_or(false, |s| {
            let id = &s.ident;
            id == "Vec"
                || id == "HashSet"
                || id == "BTreeSet"
                || id == "HashMap"
                || id == "BTreeMap"
        })
    } else {
        false
    }
}

/// Helper to get 'T' out of 'Option<T>' or return 'T' if it's not an Option.
fn extract_inner_type(ty: &Type) -> &Type {
    if let Type::Path(tp) = ty {
        if let Some(last_seg) = tp.path.segments.last() {
            if last_seg.ident == "Option" {
                if let PathArguments::AngleBracketed(args) = &last_seg.arguments {
                    if let Some(GenericArgument::Type(inner)) = args.args.first() {
                        return inner;
                    }
                }
            }
        }
    }
    ty
}

fn find_idents_in_tokens(tokens: proc_macro2::TokenStream, set: &mut HashSet<proc_macro2::Ident>) {
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

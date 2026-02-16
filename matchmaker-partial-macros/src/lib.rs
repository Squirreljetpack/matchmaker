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

    if !cfg!(feature = "partial") {
        input.attrs.retain(|attr| !attr.path().is_ident("partial"));
        if let Fields::Named(fields) = &mut input.fields {
            for field in &mut fields.named {
                field.attrs.retain(|attr| !attr.path().is_ident("partial"));
            }
        }
        return quote!(#input).into();
    }

    let name = &input.ident;
    let partial_name = format_ident!("Partial{}", name);

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let vis = &input.vis;

    let mut struct_recurse = false;
    let mut struct_unwrap = false;
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
                    } else if path.is_ident("unwrap") {
                        struct_unwrap = true;
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
                } else if meta.path.is_ident("unwrap") {
                    struct_unwrap = true;
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
    let mut flattened_field_targets = Vec::new();
    let mut used_idents = HashSet::new();

    for field in fields.iter_mut() {
        let field_name = &field.ident;
        let field_vis = &field.vis;
        let field_ty = &field.ty;

        let mut skip_field = false;
        let mut field_recurse = false;
        let mut recurse_override: Option<Option<proc_macro2::TokenStream>> = None;
        let mut field_unwrap = struct_unwrap;
        let mut field_set: Option<String> = None;
        let mut field_attrs_for_mirror = Vec::new();
        let mut field_errors = Vec::new();
        // 4 Check for Serde custom deserialization attributes
        let mut custom_deserializer: Option<Path> = None;
        let mut field_aliases = Vec::new();
        let mut is_flattened = false;

        field.attrs.retain(|attr| {
            // --- 4a. Handle #[partial] attributes ---
            if attr.path().is_ident("partial") {
                let res = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("skip") {
                        skip_field = true;
                    } else if meta.path.is_ident("unwrap") {
                        field_unwrap = true;
                    } else if meta.path.is_ident("set") {
                        let s: LitStr = meta.value()?.parse()?;
                        field_set = Some(s.value());
                    } else if meta.path.is_ident("recurse") {
                        if let Ok(value) = meta.value() {
                            let s: LitStr = value.parse().unwrap();
                            if s.value().is_empty() {
                                recurse_override = Some(None);
                            } else {
                                let ty: Type = s.parse().unwrap();
                                recurse_override = Some(Some(quote! { #ty }));
                            }
                        } else {
                            // Enable recursion using default naming convention
                            field_recurse = true;
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
                let mut drop_attr = false;
                let _ = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("deserialize_with") {
                        if let Ok(value) = meta.value() {
                            if let Ok(s) = value.parse::<LitStr>() {
                                custom_deserializer = s.parse::<Path>().ok();
                                drop_attr = true;
                            }
                        }
                    } else if meta.path.is_ident("with") {
                        if let Ok(value) = meta.value() {
                            if let Ok(s) = value.parse::<LitStr>() {
                                if let Ok(mut p) = s.parse::<Path>() {
                                    p.segments.push(format_ident!("deserialize").into());
                                    custom_deserializer = Some(p);
                                    drop_attr = true;
                                }
                            }
                        }
                    } else if meta.path.is_ident("alias") {
                        if let Ok(value) = meta.value() {
                            if let Ok(s) = value.parse::<LitStr>() {
                                field_aliases.push(s.value());
                            }
                        }
                    } else if meta.path.is_ident("flatten") {
                        is_flattened = true;
                    }
                    Ok(())
                });

                if drop_attr {
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

        if let Some(ref s) = field_set {
            if s == "sequence" && recurse_override.is_some() {
                return syn::Error::new(
                    field.span(),
                    "cannot use 'recurse' and 'set = \"sequence\"' on the same field",
                )
                .to_compile_error()
                .into();
            }
        }

        let is_opt = is_option(field_ty);
        let inner_ty = if is_opt {
            extract_inner_type_from_option(field_ty)
        } else {
            field_ty
        };

        let coll_info = get_collection_info(inner_ty);

        // Determine if we should recurse
        let mut should_recurse = (struct_recurse || field_recurse || recurse_override.is_some())
            && !matches!(recurse_override, Some(None));

        if let Some(ref s) = field_set {
            if s == "sequence" {
                should_recurse = false;
            }
        }

        let current_field_ty: proc_macro2::TokenStream;
        let mut is_recursive_field = false;

        if let Some((kind, inners)) = coll_info {
            let element_ty = inners
                .last()
                .expect("Collection must have at least one inner type");
            let partial_element_ty = if should_recurse {
                is_recursive_field = true;
                if let Some(Some(ref overridden)) = recurse_override {
                    overridden.clone()
                } else if let Type::Path(tp) = element_ty {
                    let mut p_path = tp.path.clone();
                    if let Some(seg) = p_path.segments.last_mut() {
                        seg.ident = format_ident!("Partial{}", seg.ident);
                        quote! { #p_path }
                    } else {
                        quote! { #element_ty }
                    }
                } else {
                    quote! { #element_ty }
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

            let partial_coll_ty = if inners.len() == 2 {
                let key_ty = inners[0];
                quote! { #coll_ident<#key_ty, #partial_element_ty> }
            } else {
                quote! { #coll_ident<#partial_element_ty> }
            };

            current_field_ty = if field_unwrap {
                partial_coll_ty.clone()
            } else {
                quote! { Option<#partial_coll_ty> }
            };

            // --- Apply Logic ---
            let target_expr = if is_opt {
                quote! { self.#field_name.get_or_insert_with(Default::default) }
            } else {
                quote! { self.#field_name }
            };

            let apply_stmt = if is_recursive_field {
                let element_apply = match kind {
                    CollectionKind::Vec | CollectionKind::HashSet | CollectionKind::BTreeSet => {
                        let push_method = if kind == CollectionKind::Vec {
                            quote! { push }
                        } else {
                            quote! { insert }
                        };
                        if !field_unwrap {
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
                                for p_item in partial.#field_name {
                                    let mut t = <#element_ty as Default>::default();
                                    matchmaker_partial::Apply::apply(&mut t, p_item);
                                    #target_expr.#push_method(t);
                                }
                            }
                        }
                    }
                    CollectionKind::HashMap | CollectionKind::BTreeMap => {
                        if !field_unwrap {
                            quote! {
                                for (k, p_v) in p {
                                    if let Some(v) = #target_expr.get_mut(&k) {
                                        matchmaker_partial::Apply::apply(v, p_v);
                                    } else {
                                        let mut v = <#element_ty as Default>::default();
                                        matchmaker_partial::Apply::apply(&mut v, p_v);
                                        #target_expr.insert(k, v);
                                    }
                                }
                            }
                        } else {
                            quote! {
                                for (k, p_v) in partial.#field_name {
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
                    }
                };

                if !field_unwrap {
                    quote! { if let Some(p) = partial.#field_name { #element_apply } }
                } else {
                    element_apply
                }
            } else {
                if !field_unwrap {
                    let val = if is_opt {
                        quote! { Some(p) }
                    } else {
                        quote! { p }
                    };
                    quote! { if let Some(p) = partial.#field_name { self.#field_name = #val; } }
                } else if kind == CollectionKind::HashMap || kind == CollectionKind::BTreeMap {
                    quote! {
                        for (k, v) in partial.#field_name {
                            #target_expr.insert(k, v);
                        }
                    }
                } else {
                    quote! { #target_expr.extend(partial.#field_name.into_iter()); }
                }
            };
            apply_field_stmts.push(apply_stmt);

            // --- Merge Logic ---
            if !field_unwrap {
                merge_field_stmts.push(quote! {
                    if let Some(other_coll) = other.#field_name {
                        self.#field_name.get_or_insert_with(Default::default).extend(other_coll.into_iter());
                    }
                });
                clear_field_stmts.push(quote! { self.#field_name = None; });
            } else {
                merge_field_stmts
                    .push(quote! { self.#field_name.extend(other.#field_name.into_iter()); });
                clear_field_stmts.push(quote! { self.#field_name.clear(); });
            }

            // --- Set Logic ---
            if let Some(field_ident) = &field.ident {
                let field_name_str = field_ident.to_string();
                let field_name_str = field_name_str.strip_prefix("r#").unwrap_or(&field_name_str);

                let is_sequence = field_set.as_deref() == Some("sequence");
                let set_logic = if is_sequence {
                    let assignment = if !field_unwrap {
                        quote! { self.#field_ident = Some(deserialized); }
                    } else {
                        quote! { self.#field_ident.extend(deserialized); }
                    };
                    quote! {
                        let deserialized: #partial_coll_ty = matchmaker_partial::deserialize(val)?;
                        #assignment
                    }
                } else {
                    let target = if !field_unwrap {
                        quote! { self.#field_ident.get_or_insert_with(Default::default) }
                    } else {
                        quote! { self.#field_ident }
                    };

                    let set_full_coll_logic = if !field_unwrap {
                        quote! { self.#field_ident = Some(new_map); }
                    } else {
                        quote! { #target.extend(new_map.into_iter()); }
                    };

                    if inners.len() == 2 {
                        let key_ty = inners[0];
                        let val_ty = if should_recurse {
                            quote! { #partial_element_ty }
                        } else {
                            quote! { #element_ty }
                        };

                        let descent_logic = if should_recurse {
                            quote! {
                                if rest.is_empty() {
                                    let mut combined = vec![key_str.clone()];
                                    combined.extend_from_slice(val);
                                    let (key, value): (#key_ty, #val_ty) = matchmaker_partial::deserialize(&combined)?;
                                    let _ = #target.insert(key, value);
                                } else {
                                    let key: #key_ty = matchmaker_partial::deserialize(&[key_str.clone()])?;
                                    let item = #target.entry(key).or_insert_with(Default::default);
                                    matchmaker_partial::Set::set(item, rest, val)?;
                                }
                            }
                        } else {
                            quote! {
                                if rest.is_empty() {
                                    let mut combined = vec![key_str.clone()];
                                    combined.extend_from_slice(val);
                                    let (key, value): (#key_ty, #val_ty) = matchmaker_partial::deserialize(&combined)?;
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
                        quote! {
                            if let Some((_, _)) = tail.split_first() {
                                return Err(matchmaker_partial::PartialSetError::ExtraPaths(tail.to_vec()));
                            }
                            let item: #item_ty = matchmaker_partial::deserialize(val)?;
                            #target.#push_method(item);
                        }
                    }
                };

                set_field_arms.push(quote! {
                    #field_name_str #(| #field_aliases)* => {
                        #set_logic
                        Ok(())
                    }
                });
            }
        } else {
            // Leaf field handling
            current_field_ty = if should_recurse {
                is_recursive_field = true;
                let p_ty = if let Some(Some(ref overridden)) = recurse_override {
                    overridden.clone()
                } else if let Type::Path(ty_path) = inner_ty {
                    let mut p_path = ty_path.path.clone();
                    if let Some(seg) = p_path.segments.last_mut() {
                        seg.ident = format_ident!("Partial{}", seg.ident);
                        quote! { #p_path }
                    } else {
                        quote! { #inner_ty }
                    }
                } else {
                    quote! { #inner_ty }
                };

                if field_unwrap {
                    p_ty
                } else if is_opt {
                    quote! { Option<#p_ty> }
                } else {
                    p_ty
                }
            } else if field_unwrap {
                quote! { #inner_ty }
            } else if is_opt {
                quote! { #field_ty }
            } else {
                quote! { Option<#field_ty> }
            };

            if is_recursive_field {
                if !field_unwrap && is_opt {
                    apply_field_stmts.push(quote! {
                        if let Some(p) = partial.#field_name {
                            if let Some(ref mut v) = self.#field_name {
                                matchmaker_partial::Apply::apply(v, p);
                            } else {
                                self.#field_name = Some(matchmaker_partial::from(p));
                            }
                        }
                    });
                    merge_field_stmts.push(quote! {
                        match (&mut self.#field_name, other.#field_name) {
                            (Some(s), Some(o)) => matchmaker_partial::Merge::merge(s, o),
                            (t @ None, Some(o)) => *t = Some(o),
                            _ => {}
                        }
                    });
                    clear_field_stmts.push(quote! { self.#field_name = None; });
                } else if field_unwrap && is_opt {
                    // Unwrapped recursive, base is Option
                    apply_field_stmts.push(quote! {
                        if let Some(ref mut v) = self.#field_name {
                            matchmaker_partial::Apply::apply(v, partial.#field_name);
                        } else {
                            self.#field_name = Some(matchmaker_partial::from(partial.#field_name));
                        }
                    });
                    merge_field_stmts.push(quote! { matchmaker_partial::Merge::merge(&mut self.#field_name, other.#field_name); });
                    clear_field_stmts
                        .push(quote! { matchmaker_partial::Merge::clear(&mut self.#field_name); });
                } else {
                    apply_field_stmts.push(quote! { matchmaker_partial::Apply::apply(&mut self.#field_name, partial.#field_name); });
                    merge_field_stmts.push(quote! { matchmaker_partial::Merge::merge(&mut self.#field_name, other.#field_name); });
                    clear_field_stmts
                        .push(quote! { matchmaker_partial::Merge::clear(&mut self.#field_name); });
                }

                if let Some(field_ident) = &field.ident {
                    let field_name_str = field_ident.to_string();
                    let field_name_str =
                        field_name_str.strip_prefix("r#").unwrap_or(&field_name_str);

                    let set_target = if is_opt {
                        quote! { self.#field_ident.get_or_insert_with(Default::default) }
                    } else {
                        quote! { &mut self.#field_ident }
                    };

                    if is_flattened {
                        flattened_field_targets.push(set_target);
                    } else {
                        set_field_arms.push(quote! {
                            #field_name_str #(| #field_aliases)* => {
                                if tail.is_empty() {
                                    return Err(matchmaker_partial::PartialSetError::EarlyEnd(head.clone()));
                                }
                                matchmaker_partial::Set::set(#set_target, tail, val)
                            }
                        });
                    }
                }
            } else {
                if field_unwrap {
                    if is_opt {
                        apply_field_stmts
                            .push(quote! { self.#field_name = Some(partial.#field_name); });
                    } else {
                        apply_field_stmts.push(quote! { self.#field_name = partial.#field_name; });
                    }
                } else if !is_opt {
                    apply_field_stmts.push(
                        quote! { if let Some(v) = partial.#field_name { self.#field_name = v; } },
                    );
                } else {
                    apply_field_stmts.push(
                        quote! { if let Some(v) = partial.#field_name { self.#field_name = Some(v); } },
                    );
                }
                merge_field_stmts.push(
                    quote! { if other.#field_name.is_some() { self.#field_name = other.#field_name; } },
                );
                clear_field_stmts.push(quote! { self.#field_name = None; });

                if let Some(field_ident) = &field.ident {
                    let field_name_str = field_ident.to_string();
                    let field_name_str =
                        field_name_str.strip_prefix("r#").unwrap_or(&field_name_str);

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
                        let inner_ty = extract_inner_type_from_option(field_ty);
                        quote! {
                            let deserialized = matchmaker_partial::deserialize::<#inner_ty>(val)?;
                            self.#field_name = Some(deserialized);
                        }
                    };

                    set_field_arms.push(quote! {
                        #field_name_str #(| #field_aliases)* => {
                            if !tail.is_empty() {
                                return Err(matchmaker_partial::PartialSetError::ExtraPaths(tail.to_vec()));
                            }
                            #set_logic
                            Ok(())
                        }
                    });
                }
            }
        }

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
            impl #p_impl_generics matchmaker_partial::Set for #partial_name #p_ty_generics #p_where_clause {
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

    // --- 7. Conditional Merge/Clear auto-impl ---
    let merge_impl = if enable_merge {
        quote! {
            impl #p_impl_generics matchmaker_partial::Merge for #partial_name #p_ty_generics #p_where_clause {
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

    let expanded = quote! {
        #input

        #(#final_attrs)*
        #vis struct #partial_name #p_ty_generics #p_where_clause {
            #(#partial_field_defs),*
        }

        impl #impl_generics matchmaker_partial::Apply for #name #ty_generics #where_clause {
            type Partial = #partial_name #p_ty_generics;
            fn apply(&mut self, partial: Self::Partial) {
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
        tp.path.segments.last().is_some_and(|s| s.ident == "Option")
    } else {
        false
    }
}

#[derive(PartialEq, Clone, Copy)]
enum CollectionKind {
    Vec,
    HashSet,
    BTreeSet,
    HashMap,
    BTreeMap,
}

fn get_collection_info(ty: &Type) -> Option<(CollectionKind, Vec<&Type>)> {
    if let Type::Path(tp) = ty {
        let last_seg = tp.path.segments.last()?;
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
            return None;
        };

        let mut inner_types = Vec::new();
        if let PathArguments::AngleBracketed(args) = &last_seg.arguments {
            for arg in &args.args {
                if let GenericArgument::Type(inner_ty) = arg {
                    inner_types.push(inner_ty);
                }
            }
        }
        Some((kind, inner_types))
    } else {
        None
    }
}

/// Helper to get 'T' out of 'Option<T>' or return 'T' if it's not an Option.
fn extract_inner_type_from_option(ty: &Type) -> &Type {
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

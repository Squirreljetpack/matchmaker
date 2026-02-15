use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use std::collections::HashSet;
use syn::{Fields, ItemStruct, LitStr, Meta, Path, Token, Type, parse::Parse, parse_macro_input};

#[proc_macro_attribute]
pub fn partial(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let partial_name = format_ident!("Partial{}", name);

    // Original generics for the base struct impl
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let vis = &input.vis;

    let mut struct_recurse = false;
    let mut manual_derives: Option<proc_macro2::TokenStream> = None;
    let mut manual_attrs: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut has_manual_attrs = false;

    // --- 1. Parse macro arguments ---
    if !attr.is_empty() {
        let _ = syn::parse::Parser::parse2(
            |input: syn::parse::ParseStream| {
                while !input.is_empty() {
                    let path: Path = input.parse()?;
                    if path.is_ident("recurse") {
                        struct_recurse = true;
                    } else if path.is_ident("derive") {
                        let content;
                        syn::parenthesized!(content in input);
                        let paths = content.parse_terminated(Path::parse, Token![,])?;
                        manual_derives = Some(quote! { #[derive(#paths)] });
                    } else if path.is_ident("attr") {
                        has_manual_attrs = true;
                        let content;
                        syn::parenthesized!(content in input);
                        let inner: Meta = content.parse()?;
                        manual_attrs.push(quote! { #[#inner] });
                    }
                    if input.peek(Token![,]) {
                        input.parse::<Token![,]>()?;
                    }
                }
                Ok(())
            },
            attr.into(),
        );
    }

    // --- 2. Remove any #[partial] attributes from the struct ---
    input.attrs.retain(|attr| {
        if attr.path().is_ident("partial") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("recurse") {
                    struct_recurse = true;
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
                }
                Ok(())
            });
            false
        } else {
            true
        }
    });

    // --- 3. Build final struct attributes ---
    let mut final_attrs = Vec::new();
    if let Some(manual) = manual_derives {
        final_attrs.push(manual);
    } else {
        let mut has_default = false;
        for attr in &input.attrs {
            if attr.path().is_ident("derive") {
                if attr.to_token_stream().to_string().contains("Default") {
                    has_default = true;
                }
                final_attrs.push(attr.to_token_stream());
            }
        }
        if !has_default {
            final_attrs.push(quote! { #[derive(Default)] });
        }
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
    let mut used_idents = HashSet::new();

    for field in fields.iter_mut() {
        let field_name = &field.ident;
        let field_vis = &field.vis;
        let field_ty = &field.ty;

        let mut skip_field = false;
        let mut recurse_override: Option<Option<proc_macro2::TokenStream>> = None;
        let mut field_attrs_for_mirror = Vec::new();

        field.attrs.retain(|attr| {
            if attr.path().is_ident("partial") {
                let _ = attr.parse_nested_meta(|meta| {
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
                    }
                    Ok(())
                });
                false
            } else {
                if field_attrs_for_mirror.is_empty() {
                    field_attrs_for_mirror.push(attr.to_token_stream());
                }
                true
            }
        });

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
            apply_field_stmts.push(quote! { self.#field_name.apply(partial.#field_name); });
            merge_field_stmts.push(quote! { self.#field_name.merge(other.#field_name); });
            clear_field_stmts.push(quote! { self.#field_name.clear(); });
            target_ty
        } else {
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
            actual_ty
        };

        // Track used idents for nuanced generics
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

        impl #p_impl_generics #partial_name #p_ty_generics #p_where_clause {
            pub fn merge(&mut self, other: Self) {
                #(#merge_field_stmts)*
            }

            pub fn clear(&mut self) {
                #(#clear_field_stmts)*
            }
        }
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

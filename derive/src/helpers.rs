use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    Expr, Ident, LitInt, Token, Type,
};

// --- Discriminator argument parsing (shared by instruction, account, event, program) ---

pub(crate) struct InstructionArgs {
    pub discriminator: Vec<LitInt>,
}

impl Parse for InstructionArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident: Ident = input.parse()?;
        if ident != "discriminator" {
            return Err(syn::Error::new(ident.span(), "expected `discriminator`"));
        }
        let _: Token![=] = input.parse()?;
        if input.peek(syn::token::Bracket) {
            let content;
            syn::bracketed!(content in input);
            let lits = content.parse_terminated(LitInt::parse, Token![,])?;
            let discriminator: Vec<LitInt> = lits.into_iter().collect();
            if discriminator.is_empty() {
                return Err(syn::Error::new(input.span(), "discriminator must have at least one byte"));
            }
            Ok(Self { discriminator })
        } else {
            let lit: LitInt = input.parse()?;
            Ok(Self { discriminator: vec![lit] })
        }
    }
}

// --- Type helpers ---

/// Expand a seed expression into a byte slice for use inside parse (fields are local variables).
pub(crate) fn seed_slice_expr_for_parse(expr: &Expr, field_names: &[String]) -> proc_macro2::TokenStream {
    if let Expr::Path(ep) = expr {
        if ep.path.segments.len() == 1 && ep.qself.is_none() {
            let ident = &ep.path.segments[0].ident;
            if field_names.contains(&ident.to_string()) {
                return quote! { #ident.to_account_view().address().as_ref() };
            }
        }
    }
    quote! { #expr as &[u8] }
}

/// Check if a field type's base type is `Signer`.
pub(crate) fn is_signer_type(ty: &Type) -> bool {
    let inner = match ty {
        Type::Reference(r) => &*r.elem,
        other => other,
    };
    if let Type::Path(p) = inner {
        if let Some(last) = p.path.segments.last() {
            return last.ident == "Signer";
        }
    }
    false
}

pub(crate) fn strip_generics(ty: &Type) -> proc_macro2::TokenStream {
    match ty {
        Type::Path(type_path) => {
            let segments: Vec<_> = type_path
                .path
                .segments
                .iter()
                .map(|seg| &seg.ident)
                .collect();
            quote! { #(#segments)::* }
        }
        _ => panic!("Unsupported field type"),
    }
}

pub(crate) fn pascal_to_snake(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_lowercase().next().unwrap());
    }
    result
}

pub(crate) fn snake_to_pascal(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + &chars.collect::<String>(),
            }
        })
        .collect()
}

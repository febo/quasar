use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, FnArg, Ident, ItemFn, Pat, Type,
};

use crate::helpers::InstructionArgs;

pub(crate) fn instruction(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as InstructionArgs);
    let mut func = parse_macro_input!(item as ItemFn);
    let disc_bytes = &args.discriminator;
    let disc_len = disc_bytes.len();

    let first_arg = match func.sig.inputs.first() {
        Some(FnArg::Typed(pt)) => pt.clone(),
        _ => panic!("#[instruction] requires ctx: Ctx<T> as first parameter"),
    };

    let param_name = &first_arg.pat;
    let param_ident = match &*first_arg.pat {
        Pat::Ident(pat_ident) => pat_ident.ident.clone(),
        _ => panic!("#[instruction] ctx parameter must be an identifier"),
    };
    let param_type = &first_arg.ty;

    let remaining: Vec<_> = func.sig.inputs.iter().skip(1).filter_map(|arg| {
        match arg {
            FnArg::Typed(pt) => Some(pt.clone()),
            _ => None,
        }
    }).collect();

    func.sig.inputs = syn::punctuated::Punctuated::new();
    func.sig.inputs.push(syn::parse_quote!(mut context: Context));

    let stmts = std::mem::take(&mut func.block.stmts);
    let mut new_stmts: Vec<syn::Stmt> = vec![
        syn::parse_quote!(
            if !context.data.starts_with(&[#(#disc_bytes),*]) {
                return Err(ProgramError::InvalidInstructionData);
            }
        ),
        syn::parse_quote!(
            context.data = &context.data[#disc_len..];
        ),
        syn::parse_quote!(
            let mut #param_name: #param_type = Ctx::new(context)?;
        ),
    ];

    if !remaining.is_empty() {
        let field_names: Vec<Ident> = remaining.iter().map(|pt| {
            match &*pt.pat {
                Pat::Ident(pat_ident) => pat_ident.ident.clone(),
                _ => panic!("#[instruction] parameters must be simple identifiers"),
            }
        }).collect();

        let field_types: Vec<&Type> = remaining.iter().map(|pt| &*pt.ty).collect();

        new_stmts.push(syn::parse_quote!(
            #[repr(C)]
            struct InstructionData {
                #(#field_names: #field_types,)*
            }
        ));

        new_stmts.push(syn::parse_quote!(
            if #param_ident.data.len() < core::mem::size_of::<InstructionData>() {
                return Err(ProgramError::InvalidInstructionData);
            }
        ));

        new_stmts.push(syn::parse_quote!(
            let __instruction_data = unsafe { core::ptr::read_unaligned(#param_ident.data.as_ptr() as *const InstructionData) };
        ));

        for name in &field_names {
            new_stmts.push(syn::parse_quote!(
                let #name = __instruction_data.#name;
            ));
        }
    }

    func.block.stmts = new_stmts.into_iter().chain(stmts).collect();

    quote!(#func).into()
}

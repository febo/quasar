use proc_macro::TokenStream;
use quote::{quote, format_ident};
use syn::{
    parse_macro_input, Data, DeriveInput, Fields, Type,
};

use crate::helpers::InstructionArgs;

fn map_to_pod_type(ty: &Type) -> proc_macro2::TokenStream {
    if let Type::Path(type_path) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            let ident_str = seg.ident.to_string();
            return match ident_str.as_str() {
                "u128" => quote! { quasar_core::pod::PodU128 },
                "u64" => quote! { quasar_core::pod::PodU64 },
                "u32" => quote! { quasar_core::pod::PodU32 },
                "u16" => quote! { quasar_core::pod::PodU16 },
                "i128" => quote! { quasar_core::pod::PodI128 },
                "i64" => quote! { quasar_core::pod::PodI64 },
                "i32" => quote! { quasar_core::pod::PodI32 },
                "i16" => quote! { quasar_core::pod::PodI16 },
                "bool" => quote! { quasar_core::pod::PodBool },
                _ => quote! { #ty },
            };
        }
    }
    quote! { #ty }
}

pub(crate) fn account(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as InstructionArgs);
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;
    let disc_bytes = &args.discriminator;
    let disc_len = disc_bytes.len();

    let disc_values: Vec<u8> = disc_bytes.iter()
        .map(|lit| lit.base10_parse::<u8>().expect("discriminator byte must be 0-255"))
        .collect();
    if disc_values.iter().all(|&b| b == 0) {
        return syn::Error::new_spanned(
            &args.discriminator[0],
            "discriminator must contain at least one non-zero byte; all-zero discriminators are indistinguishable from uninitialized account data",
        ).to_compile_error().into();
    }

    let disc_indices: Vec<usize> = (0..disc_len).collect();

    let fields_data = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("#[account] can only be used on structs with named fields"),
        },
        _ => panic!("#[account] can only be used on structs"),
    };

    let field_types: Vec<_> = fields_data.iter().map(|f| &f.ty).collect();

    let zc_name = format_ident!("{}Zc", name);
    let zc_fields: Vec<proc_macro2::TokenStream> = fields_data.iter().map(|f| {
        let fname = &f.ident;
        let vis = &f.vis;
        let zc_ty = map_to_pod_type(&f.ty);
        quote! { #vis #fname: #zc_ty }
    }).collect();

    quote! {
        #[repr(C)]
        #[derive(::wincode::SchemaRead, ::wincode::SchemaWrite)]
        #input

        #[repr(C)]
        #[derive(Copy, Clone)]
        pub struct #zc_name {
            #(#zc_fields,)*
        }

        const _: () = assert!(
            core::mem::align_of::<#zc_name>() == 1,
            "ZC companion struct must have alignment 1; all fields must use Pod types or alignment-1 types"
        );

        impl Discriminator for #name {
            const DISCRIMINATOR: &'static [u8] = &[#(#disc_bytes),*];
        }

        impl Space for #name {
            const SPACE: usize = #disc_len #(+ core::mem::size_of::<#field_types>())*;
        }

        impl Owner for #name {
            const OWNER: Address = crate::ID;
        }

        impl AccountCheck for #name {
            #[inline(always)]
            fn check(view: &AccountView) -> Result<(), ProgramError> {
                let __data = unsafe { view.borrow_unchecked() };
                if __data.len() < #disc_len + core::mem::size_of::<#zc_name>() {
                    return Err(ProgramError::AccountDataTooSmall);
                }
                #(
                    if unsafe { *__data.get_unchecked(#disc_indices) } != #disc_bytes {
                        return Err(ProgramError::InvalidAccountData);
                    }
                )*
                Ok(())
            }
        }

        impl ZeroCopyDeref for #name {
            type Target = #zc_name;
            const DATA_OFFSET: usize = Self::DISCRIMINATOR.len();
        }

        impl QuasarAccount for #name {
            #[inline(always)]
            fn deserialize(data: &[u8]) -> Result<Self, ProgramError> {
                ::wincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)
            }

            #[inline(always)]
            fn serialize(&self, data: &mut [u8]) -> Result<(), ProgramError> {
                ::wincode::serialize_into(data, self).map_err(|_| ProgramError::InvalidAccountData)
            }
        }

        impl #name {
            #[inline(always)]
            pub fn init(self, account: &mut Initialize<Self>, payer: &AccountView, rent: Option<&Rent>) -> Result<(), ProgramError> {
                self.init_signed(account, payer, rent, &[])
            }

            #[inline(always)]
            pub fn init_signed(self, account: &mut Initialize<Self>, payer: &AccountView, rent: Option<&Rent>, signers: &[quasar_core::cpi::Signer]) -> Result<(), ProgramError> {
                let view = account.to_account_view();

                {
                    let __existing = unsafe { view.borrow_unchecked() };
                    if __existing.len() >= Self::DISCRIMINATOR.len()
                        && __existing[..Self::DISCRIMINATOR.len()] == *Self::DISCRIMINATOR
                    {
                        return Err(QuasarError::AccountAlreadyInitialized.into());
                    }
                }

                use quasar_core::sysvars::Sysvar;
                let lamports = match rent {
                    Some(rent_account) => rent_account.get()?.try_minimum_balance(Self::SPACE)?,
                    None => quasar_core::sysvars::rent::Rent::get()?.try_minimum_balance(Self::SPACE)?,
                };

                if view.lamports() == 0 {
                    quasar_core::cpi::system::create_account(payer, view, lamports, Self::SPACE as u64, &Self::OWNER)
                        .invoke_with_signers(signers)?;
                } else {
                    let required = lamports.saturating_sub(view.lamports());
                    if required > 0 {
                        quasar_core::cpi::system::transfer(payer, view, required)
                            .invoke_with_signers(signers)?;
                    }
                    quasar_core::cpi::system::assign(view, &Self::OWNER)
                        .invoke_with_signers(signers)?;
                    unsafe { view.resize_unchecked(Self::SPACE) }?;
                }

                let mut data = view.try_borrow_mut()?;
                data[..Self::DISCRIMINATOR.len()].copy_from_slice(Self::DISCRIMINATOR);
                self.serialize(&mut data[Self::DISCRIMINATOR.len()..])?;
                Ok(())
            }
        }
    }.into()
}

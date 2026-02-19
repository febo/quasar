use proc_macro::TokenStream;
use quote::{quote, format_ident};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Data, DeriveInput, Expr, ExprArray, Fields, Ident, Token, Type,
};

use crate::helpers::{seed_slice_expr_for_parse, is_signer_type, strip_generics, pascal_to_snake};

// --- Account field attribute parsing ---

enum AccountDirective {
    HasOne(Ident),
    Constraint(Expr),
    Seeds(Vec<Expr>),
    Bump(Option<Expr>),
}

impl Parse for AccountDirective {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key: Ident = input.parse()?;
        match key.to_string().as_str() {
            "has_one" => {
                let _: Token![=] = input.parse()?;
                Ok(Self::HasOne(input.parse()?))
            }
            "constraint" => {
                let _: Token![=] = input.parse()?;
                Ok(Self::Constraint(input.parse()?))
            }
            "seeds" => {
                let _: Token![=] = input.parse()?;
                let arr: ExprArray = input.parse()?;
                Ok(Self::Seeds(arr.elems.into_iter().collect()))
            }
            "bump" => {
                if input.peek(Token![=]) {
                    let _: Token![=] = input.parse()?;
                    Ok(Self::Bump(Some(input.parse()?)))
                } else {
                    Ok(Self::Bump(None))
                }
            }
            _ => Err(syn::Error::new(
                key.span(),
                format!("unknown account attribute: `{}`", key),
            )),
        }
    }
}

struct AccountFieldAttrs {
    has_ones: Vec<Ident>,
    constraints: Vec<Expr>,
    seeds: Option<Vec<Expr>>,
    bump: Option<Option<Expr>>,
}

impl Parse for AccountFieldAttrs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let directives =
            input.parse_terminated(AccountDirective::parse, Token![,])?;
        let mut has_ones = Vec::new();
        let mut constraints = Vec::new();
        let mut seeds = None;
        let mut bump = None;
        for d in directives {
            match d {
                AccountDirective::HasOne(ident) => has_ones.push(ident),
                AccountDirective::Constraint(expr) => constraints.push(expr),
                AccountDirective::Seeds(s) => seeds = Some(s),
                AccountDirective::Bump(b) => bump = Some(b),
            }
        }
        Ok(Self { has_ones, constraints, seeds, bump })
    }
}

fn parse_field_attrs(field: &syn::Field) -> AccountFieldAttrs {
    for attr in &field.attrs {
        if attr.path().is_ident("account") {
            return attr
                .parse_args::<AccountFieldAttrs>()
                .expect("failed to parse #[account(...)] attribute");
        }
    }
    AccountFieldAttrs {
        has_ones: vec![],
        constraints: vec![],
        seeds: None,
        bump: None,
    }
}

// --- Derive Accounts ---

pub(crate) fn derive_accounts(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let bumps_name = format_ident!("{}Bumps", name);

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Accounts can only be derived for structs with named fields"),
        },
        _ => panic!("Accounts can only be derived for structs"),
    };

    let field_names: Vec<_> = fields.iter().map(|f| &f.ident).collect();

    let field_constructs: Vec<proc_macro2::TokenStream> = fields.iter().map(|f| {
        let name = &f.ident;
        match &f.ty {
            Type::Reference(type_ref) => {
                let base_type = strip_generics(&type_ref.elem);
                if type_ref.mutability.is_some() {
                    quote! { #name: #base_type::from_account_view_mut(#name)? }
                } else {
                    quote! { #name: #base_type::from_account_view(#name)? }
                }
            }
            _ => {
                let base_type = strip_generics(&f.ty);
                quote! { #name: #base_type::from_account_view(#name)? }
            }
        }
    }).collect();

    let field_name_strings: Vec<String> = fields.iter()
        .filter_map(|f| f.ident.as_ref().map(|i| i.to_string()))
        .collect();

    let mut has_one_checks: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut constraint_checks: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut pda_checks: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut bump_init_vars: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut bump_struct_fields: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut bump_struct_inits: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut seeds_methods: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut seed_addr_captures: Vec<proc_macro2::TokenStream> = Vec::new();

    for field in fields.iter() {
        let attrs = parse_field_attrs(field);
        let field_name = field.ident.as_ref().unwrap();

        for target in &attrs.has_ones {
            has_one_checks.push(quote! {
                if #field_name.#target != *#target.to_account_view().address() {
                    return Err(QuasarError::HasOneMismatch.into());
                }
            });
        }

        for expr in &attrs.constraints {
            constraint_checks.push(quote! {
                if !(#expr) {
                    return Err(QuasarError::ConstraintViolation.into());
                }
            });
        }

        if let Some(ref seed_exprs) = attrs.seeds {
            let bump_var = format_ident!("__bumps_{}", field_name);

            bump_init_vars.push(quote! { let mut #bump_var: u8 = 0; });
            bump_struct_fields.push(quote! { pub #field_name: u8 });
            bump_struct_inits.push(quote! { #field_name: #bump_var });

            let bump_arr_field = format_ident!("__{}_bump", field_name);
            bump_struct_fields.push(quote! { #bump_arr_field: [u8; 1] });
            bump_struct_inits.push(quote! { #bump_arr_field: [#bump_var] });

            let seed_slices: Vec<proc_macro2::TokenStream> = seed_exprs.iter().map(|expr| {
                seed_slice_expr_for_parse(expr, &field_name_strings)
            }).collect();

            let seed_idents: Vec<Ident> = seed_slices.iter().enumerate().map(|(idx, _)| {
                format_ident!("__seed_{}_{}", field_name, idx)
            }).collect();

            let seed_len_checks: Vec<proc_macro2::TokenStream> = seed_idents
                .iter()
                .zip(seed_slices.iter())
                .map(|(ident, seed)| {
                    quote! {
                        let #ident: &[u8] = #seed;
                        if #ident.len() > 32 {
                            return Err(QuasarError::InvalidSeeds.into());
                        }
                    }
                })
                .collect();

            match &attrs.bump {
                Some(Some(bump_expr)) => {
                    pda_checks.push(quote! {
                        {
                            #(#seed_len_checks)*
                            let __bump_val: u8 = #bump_expr;
                            let __bump_ref: &[u8] = &[__bump_val];
                            let __pda_seeds = [#(quasar_core::cpi::Seed::from(#seed_idents),)* quasar_core::cpi::Seed::from(__bump_ref)];
                            let __expected = quasar_core::pda::create_program_address(&__pda_seeds, &crate::ID)?;
                            if *#field_name.to_account_view().address() != __expected {
                                return Err(QuasarError::InvalidPda.into());
                            }
                            #bump_var = __bump_val;
                        }
                    });
                }
                Some(None) => {
                    pda_checks.push(quote! {
                        {
                            #(#seed_len_checks)*
                            let __pda_seeds = [#(quasar_core::cpi::Seed::from(#seed_idents)),*];
                            let (__expected, __bump) = quasar_core::pda::find_program_address(&__pda_seeds, &crate::ID);
                            if *#field_name.to_account_view().address() != __expected {
                                return Err(QuasarError::InvalidPda.into());
                            }
                            #bump_var = __bump;
                        }
                    });
                }
                None => {
                    panic!("#[account(seeds = [...])] requires a `bump` or `bump = expr` directive");
                }
            }

            let method_name = format_ident!("{}_seeds", field_name);
            let seed_count = seed_exprs.len() + 1;
            let mut seed_elements: Vec<proc_macro2::TokenStream> = Vec::new();

            for expr in seed_exprs {
                if let Expr::Path(ep) = expr {
                    if ep.qself.is_none() && ep.path.segments.len() == 1 {
                        let ident = &ep.path.segments[0].ident;
                        if field_name_strings.contains(&ident.to_string()) {
                            let addr_field = format_ident!("__seed_{}_{}", field_name, ident);
                            let capture_var = format_ident!("__seed_addr_{}_{}", field_name, ident);

                            seed_addr_captures.push(quote! {
                                let #capture_var = *#ident.address();
                            });
                            bump_struct_fields.push(quote! { #addr_field: Address });
                            bump_struct_inits.push(quote! { #addr_field: #capture_var });

                            seed_elements.push(quote! { quasar_core::cpi::Seed::from(self.#addr_field.as_ref()) });
                            continue;
                        }
                    }
                }
                seed_elements.push(quote! { quasar_core::cpi::Seed::from((#expr) as &[u8]) });
            }

            seed_elements.push(quote! { quasar_core::cpi::Seed::from(&self.#bump_arr_field as &[u8]) });

            seeds_methods.push(quote! {
                #[inline(always)]
                pub fn #method_name(&self) -> [quasar_core::cpi::Seed<'_>; #seed_count] {
                    [#(#seed_elements),*]
                }
            });
        }
    }

    let field_count = field_names.len();
    let field_indices: Vec<usize> = (0..field_count).collect();

    let parse_steps: Vec<proc_macro2::TokenStream> = field_indices.iter().map(|&i| {
        quote! {
            {
                let raw = input as *mut quasar_core::__private::RuntimeAccount;
                if unsafe { (*raw).borrow_state } == quasar_core::__private::NOT_BORROWED {
                    unsafe {
                        core::ptr::write(base.add(#i), quasar_core::__private::AccountView::new_unchecked(raw));
                        input = input.add(__ACCOUNT_HEADER + (*raw).data_len as usize);
                        let addr = input as usize;
                        input = ((addr + 7) & !7) as *mut u8;
                    }
                } else {
                    unsafe {
                        let idx = (*raw).borrow_state as usize;
                        core::ptr::write(base.add(#i), core::ptr::read(base.add(idx)));
                        input = input.add(core::mem::size_of::<u64>());
                    }
                }
            }
        }
    }).collect();

    let has_pda_fields = !bump_struct_fields.is_empty();

    let bumps_struct = if has_pda_fields {
        quote! { #[derive(Copy, Clone)] pub struct #bumps_name { #(#bump_struct_fields,)* } }
    } else {
        quote! { #[derive(Copy, Clone)] pub struct #bumps_name; }
    };

    let bumps_init = if has_pda_fields {
        quote! { #bumps_name { #(#bump_struct_inits,)* } }
    } else {
        quote! { #bumps_name }
    };

    let has_any_checks = !has_one_checks.is_empty()
        || !constraint_checks.is_empty()
        || !pda_checks.is_empty();

    let parse_body = if has_any_checks {
        quote! {
            let [#(#field_names),*] = accounts else {
                return Err(ProgramError::NotEnoughAccountKeys);
            };

            #(#seed_addr_captures)*

            let result = Self {
                #(#field_constructs,)*
            };

            #(#bump_init_vars)*

            {
                let Self { #(ref #field_names,)* } = result;
                #(#has_one_checks)*
                #(#constraint_checks)*
                #(#pda_checks)*
            }

            Ok((result, #bumps_init))
        }
    } else {
        quote! {
            let [#(#field_names),*] = accounts else {
                return Err(ProgramError::NotEnoughAccountKeys);
            };

            Ok((Self {
                #(#field_constructs,)*
            }, #bumps_init))
        }
    };

    let seeds_impl = if seeds_methods.is_empty() {
        quote! {}
    } else {
        quote! {
            impl #bumps_name {
                #(#seeds_methods)*
            }
        }
    };

    // --- Client instruction macro (off-chain only) ---
    // Generates a #[macro_export] macro that the #[program] macro invokes
    // to produce flat instruction structs with account + arg fields.
    let snake_name = pascal_to_snake(&name.to_string());
    let macro_name_str = format!("__{}_instruction", snake_name);

    let account_fields_str: String = fields.iter().map(|f| {
        let field_name = f.ident.as_ref().unwrap().to_string();
        format!("pub {}: solana_address::Address,", field_name)
    }).collect::<Vec<_>>().join("\n                ");

    let account_metas_str: String = fields.iter().map(|f| {
        let field_name = f.ident.as_ref().unwrap().to_string();
        let writable = matches!(&f.ty, Type::Reference(r) if r.mutability.is_some());
        let signer = is_signer_type(&f.ty);
        if writable {
            format!("quasar_core::client::AccountMeta::new(ix.{}, {}),", field_name, signer)
        } else {
            format!("quasar_core::client::AccountMeta::new_readonly(ix.{}, {}),", field_name, signer)
        }
    }).collect::<Vec<_>>().join("\n                        ");

    let macro_def_str = format!(
        r#"
        #[cfg(not(any(target_arch = "bpf", target_os = "solana")))]
        #[doc(hidden)]
        #[macro_export]
        macro_rules! {macro_name} {{
            ($struct_name:ident, [$($disc:expr),*], {{$($arg_name:ident : $arg_ty:ty),*}}) => {{
                pub struct $struct_name {{
                    {account_fields}
                    $(pub $arg_name: $arg_ty,)*
                }}

                impl From<$struct_name> for quasar_core::client::Instruction {{
                    fn from(ix: $struct_name) -> quasar_core::client::Instruction {{
                        let accounts = vec![
                            {account_metas}
                        ];
                        let data = quasar_core::client::build_instruction_data(
                            &[$($disc),*],
                            |_data| {{ $(quasar_core::client::WriteBytes::write_bytes(&ix.$arg_name, _data);)* }}
                        );
                        quasar_core::client::Instruction {{
                            program_id: crate::ID,
                            accounts,
                            data,
                        }}
                    }}
                }}
            }};
        }}
        "#,
        macro_name = macro_name_str,
        account_fields = account_fields_str,
        account_metas = account_metas_str,
    );

    let client_macro: proc_macro2::TokenStream = macro_def_str.parse()
        .expect("failed to parse client instruction macro");

    let expanded = quote! {
        #bumps_struct

        impl<'info> ParseAccounts<'info> for #name<'info> {
            type Bumps = #bumps_name;

            #[inline(always)]
            fn parse(accounts: &'info [AccountView]) -> Result<(Self, Self::Bumps), ProgramError> {
                #parse_body
            }
        }

        #seeds_impl

        impl<'info> AccountCount for #name<'info> {
            const COUNT: usize = #field_count;
        }

        impl<'info> #name<'info> {
            #[inline(always)]
            pub unsafe fn parse_accounts(
                mut input: *mut u8,
                buf: &mut core::mem::MaybeUninit<[quasar_core::__private::AccountView; #field_count]>,
            ) -> *mut u8 {
                const __ACCOUNT_HEADER: usize =
                    core::mem::size_of::<quasar_core::__private::RuntimeAccount>()
                    + quasar_core::__private::MAX_PERMITTED_DATA_INCREASE
                    + core::mem::size_of::<u64>();

                let base = buf.as_mut_ptr() as *mut quasar_core::__private::AccountView;

                #(#parse_steps)*

                input
            }
        }

        #client_macro
    };

    TokenStream::from(expanded)
}

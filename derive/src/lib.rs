use proc_macro::TokenStream;

mod helpers;
mod accounts;
mod instruction;
mod account;
mod program;
mod event;
mod error_code;

#[proc_macro_derive(Accounts, attributes(account))]
pub fn derive_accounts(input: TokenStream) -> TokenStream {
    accounts::derive_accounts(input)
}

#[proc_macro_attribute]
pub fn instruction(attr: TokenStream, item: TokenStream) -> TokenStream {
    instruction::instruction(attr, item)
}

#[proc_macro_attribute]
pub fn account(attr: TokenStream, item: TokenStream) -> TokenStream {
    account::account(attr, item)
}

#[proc_macro_attribute]
pub fn program(attr: TokenStream, item: TokenStream) -> TokenStream {
    program::program(attr, item)
}

#[proc_macro_attribute]
pub fn event(attr: TokenStream, item: TokenStream) -> TokenStream {
    event::event(attr, item)
}

#[proc_macro_attribute]
pub fn error_code(attr: TokenStream, item: TokenStream) -> TokenStream {
    error_code::error_code(attr, item)
}

#[proc_macro]
pub fn emit_cpi(input: TokenStream) -> TokenStream {
    let input = proc_macro2::TokenStream::from(input);
    quote::quote! {
        self.program.emit_event(&#input, self.event_authority)
    }.into()
}

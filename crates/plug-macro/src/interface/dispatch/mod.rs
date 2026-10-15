use std::ops::Deref;

use proc_macro2::TokenStream;
use syn::{
    Path, Result, Token,
    parse::{Parse, ParseStream},
};

use crate::{
    interface::{InterfaceShape, Mode},
    parse::Many,
};

pub mod direct;
pub mod dynamic;

impl Deref for super::Mode {
    type Target = DispatchMode;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Dynamic => &DispatchMode::Dynamic,
            _ => &DispatchMode::Direct,
        }
    }
}

#[derive(Default)]
pub enum DispatchMode {
    /// similar to enum-dispatch
    #[default]
    Direct,

    /// similar to rustc's trait objects
    Dynamic,
}

impl DispatchMode {
    pub fn supports_const(&self) -> bool {
        matches!(self, Self::Direct)
    }
}

pub struct DispatchCode {
    pub all_tags: TokenStream,
    pub object: TokenStream,
    pub dispatched_methods: Vec<TokenStream>,
    pub other_codegen: Option<TokenStream>,
}

//

pub struct DispatchTarget {
    mode: Mode,
    paths: Option<Vec<Path>>,
}

impl Deref for DispatchTarget {
    type Target = Mode;

    fn deref(&self) -> &Self::Target {
        &self.mode
    }
}

impl Parse for DispatchTarget {
    fn parse(input: ParseStream) -> Result<Self> {
        let mode_parse = input.lookahead1();

        let mode = if mode_parse.peek(Token![mod]) {
            input.parse::<Token![mod]>()?;
            Mode::FromMod
        } else if mode_parse.peek(Token![dyn]) {
            input.parse::<Token![dyn]>()?;
            Mode::Dynamic
        } else {
            Mode::Direct
        };

        let paths: Option<Many<Path>> = match mode {
            Mode::Direct => Some(input.parse()?),
            Mode::FromMod => {
                let content;
                syn::parenthesized!(content in input);
                Some(content.parse()?)
            }
            Mode::Dynamic => None,
        };

        let paths = paths.map(|x| x.inner);

        Ok(DispatchTarget { mode, paths })
    }
}

pub fn dispatch(shape: InterfaceShape) -> Result<DispatchCode> {
    match **shape.dispatch_target {
        DispatchMode::Dynamic => todo!("dyn path"),
        DispatchMode::Direct => direct::dispatch(shape),
    }
}

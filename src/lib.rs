mod link;

use std::ops::Deref;

use dyn_utils::{
    DynObject,
    object::DynTrait,
    storage::{DefaultStorage, Storage},
};

use facet::Facet;

hazymacros::setup!(extern _h);
use hazymacros::trait_alias;

#[doc(hidden)]
pub mod __codegen {
    pub use dyn_utils;
    pub use facet;
    pub use inventory;
    pub use paste;
}

pub use dyn_utils::storage;

#[derive(Facet)]
pub struct ConfigCell<T: Pluggable> {
    #[facet(flatten)]
    pub common: T::CommonConfig,
    #[facet(flatten)]
    specific: T,
}

// Make specific configuration available by default
impl<T: Pluggable> Deref for ConfigCell<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.specific
    }
}

trait_alias!(pub FromConfig: for<'a> Facet<'a>);

pub trait Pluggable: Sized + DynTrait {
    type CommonConfig: FromConfig;
    fn plug<C>(config: &C) -> Option<ConfigCell<Self>>;
}

pub struct Plugged<T: Pluggable, S: Storage = DefaultStorage> {
    object: DynObject<T, S>,
}

// TODO: we can maybe get away without `dyn` at all!
// this will require enum_dispatch and listing all impls on define explicitly
// the latter may be made amenable with plug_mod

// TODO: do not require Interface in scope to call by Object
// ref: `inherent` crate

// TODO: we can cut `paste` dependency if we manually write DynTrait and run `dyn_object` directly on it
// which seems doable and even reasonable
#[macro_export]
macro_rules! define {
    ($vis:vis trait $name:ident { $($body:tt)* }) => {
        $crate::_h::hazymacros::purescope! { $vis $name {
            use $crate::__codegen::*;
            use facet::Shape;
            use scattered_collect::{gather, slice::ScatteredSlice as LinkSlice};

            #[dyn_trait]
            #[dyn_trait(dyn_object)]
            pub trait Interface { $($body)* }

            paste::paste! { pub type Object = dyn_utils::object::DynObject<dyn [<Dyn $name>]> }

            #[doc(hidden)]
            pub struct Link(fn() -> &'static Shape);
            inventory::collect!(Link);
        }}
    };
}

#[macro_export]
macro_rules! register {
    ($interface:path => $impl:path) => {
        $crate::__codegen::inventory::submit!($interface::Link(|| {
            &<$impl as $crate::__codegen::facet::Facet>::SHAPE
        }));
    };
}

struct Test;

// TODO: consider dtonlay linkme

type LinkedShape = fn() -> &'static facet::Shape;

#[cfg(debug_assertions)]
const fn shape<T, Link: inventory::Collect + Into<LinkedShape>>() -> facet::Shape {
    use std::mem;

    use facet::{Def, Field, FieldFlags, Repr, ShapeRef, StructKind, StructType, Type};

    let fields: Vec<Field> = inventory::iter::<Link>
        .into_iter()
        .map(|shape| Field {
            name: "shape.identifier to proper case",

            shape: ShapeRef((*shape).into()),

            offset: 0, /* TODO */
            flags: FieldFlags::CHILD,

            rename: None,
            alias: None,
            attributes: &[],
            doc: &[],
            skip_serializing_if: None,
            default: None,
            invariants: None,
            proxy: None,
            format_proxies: &[],
            metadata: None,
        })
        .collect();

    let fields = fields.leak();

    let definition = StructType {
        repr: Repr::default(),
        kind: StructKind::Struct,
        fields: &fields,
    };

    facet::Shape::builder_for_unsized::<T>("Config")
        .def(Def::Undefined)
        .ty(Type::User(facet::UserType::Struct(definition)))
        .build()
}

// macro_rules! root {
//     () => {};
// }

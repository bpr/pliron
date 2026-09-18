// SPDX-License-Identifier: Apache-2.0
// Copyright (c) The pliron contributors

//! Store, in [Context], a single unique copy of any object.
//!
//! [save] / [get] / [UniquedKey] are the raw store. [Uniqued] wraps a key so that
//! a value handled by identity can be a field of an [Attribute](crate::attribute::Attribute),
//! [Type](crate::type::Type) or any other IR entity, with printing, parsing and
//! decontextualization delegated to the stored value.

use core::{
    any::Any,
    fmt::{self, Debug},
    hash::{Hash, Hasher},
    marker::PhantomData,
};

use alloc::boxed::Box;

use crate::{
    combine::Parser,
    context::Context,
    irbuild::decontext::{CloneIntoContext, StableHash},
    parsable::{Parsable, ParseResult, StateStream},
    printable::{self, Printable},
    storage_uniquer::TypeValueHash,
};

/// [Box]ed [Any], used for unique storage.
pub(crate) struct UniquedAny(Box<dyn Any + Send>);

/// A handle to the stored unique copy of an object.
#[derive(Debug)]
pub struct UniquedKey<T> {
    index: usize,
    _dummy: PhantomData<T>,
}

impl<T> Clone for UniquedKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for UniquedKey<T> {}

// Manual impls, as for `Clone`: a key is an index, so equality must not
// require `T: PartialEq`.
impl<T> PartialEq for UniquedKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<T> Eq for UniquedKey<T> {}

impl<T: 'static> Hash for UniquedKey<T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        core::any::TypeId::of::<T>().hash(state);
    }
}

/// Save a unique copy of an object and get a handle to the saved copy.
pub fn save<T: Any + Hash + Eq + Send>(ctx: &mut Context, t: T) -> UniquedKey<T> {
    let hash = TypeValueHash::new(&t);
    let t = UniquedAny(Box::new(t));
    let eq = |t1: &UniquedAny, t2: &UniquedAny| -> bool {
        t1.0.downcast_ref::<T>() == t2.0.downcast_ref::<T>()
    };
    UniquedKey {
        index: ctx.uniqued_any_store.get_or_create_unique(t, hash, &eq),
        _dummy: PhantomData,
    }
}

/// Given a handle to a stored unique copy of an object, get a reference to the object itself.
pub fn get<T: Any + Hash + Eq>(ctx: &Context, key: UniquedKey<T>) -> &T {
    ctx.uniqued_any_store
        .unique_store
        .get(key.index)
        .expect("Key not found in uniqued store")
        .0
        .downcast_ref::<T>()
        .expect("Type mismatch in uniqued store")
}

/// A value stored once in the [Context] and handled by identity.
///
/// Two `Uniqued<T>` built (in the same [Context]) from equal values are equal,
/// and [Hash] / [PartialEq] compare the handle rather than the value. This is
/// how a [Type](crate::type::Type) is handled, made available to any `T`:
/// canonicalize the value once, then compare handles.
///
/// The typical use is an [Attribute](crate::attribute::Attribute) whose payload is
/// an expression tree with a canonical form. Comparing the tree structurally on
/// every equality check (as a plain field would) is a walk; comparing a `Uniqued`
/// field is an integer compare. Attributes are otherwise not uniqued, see the
/// [attribute](crate::attribute) module.
///
/// [Printable], [Parsable], [StableHash] and [CloneIntoContext] all delegate to
/// the stored value, so a `Uniqued<T>` field needs nothing beyond what `T`
/// already implements. Parsing re-uniques the parsed value into the parsing
/// [Context], and cloning into another [Context] re-uniques there, so identity
/// holds within a [Context] and is never carried across one.
///
/// ```
/// use pliron::{context::Context, uniqued_any::Uniqued};
///
/// let ctx = &mut Context::new();
/// let a = Uniqued::new(ctx, String::from("x + 1"));
/// let b = Uniqued::new(ctx, String::from("x + 1"));
/// let c = Uniqued::new(ctx, String::from("x + 2"));
/// assert_eq!(a, b);
/// assert_ne!(a, c);
/// assert_eq!(a.get(ctx), "x + 1");
/// ```
pub struct Uniqued<T>(UniquedKey<T>);

impl<T: Any + Hash + Eq + Send> Uniqued<T> {
    /// Store `value` (or find the copy already stored) and get a handle to it.
    pub fn new(ctx: &mut Context, value: T) -> Self {
        Self(save(ctx, value))
    }

    /// The stored value.
    pub fn get<'c>(&self, ctx: &'c Context) -> &'c T {
        get(ctx, self.0)
    }

    /// The underlying store key.
    pub fn key(&self) -> UniquedKey<T> {
        self.0
    }
}

impl<T> Clone for Uniqued<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Uniqued<T> {}

impl<T> PartialEq for Uniqued<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T> Eq for Uniqued<T> {}

impl<T: 'static> Hash for Uniqued<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T> Debug for Uniqued<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Uniqued").field(&self.0.index).finish()
    }
}

impl<T: Any + Hash + Eq + Send + Printable> Printable for Uniqued<T> {
    fn fmt(
        &self,
        ctx: &Context,
        state: &printable::State,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        self.get(ctx).fmt(ctx, state, f)
    }
}

impl<T> Parsable for Uniqued<T>
where
    T: Any + Hash + Eq + Send + Parsable<Arg = (), Parsed = T>,
{
    type Arg = ();
    type Parsed = Self;

    fn parse<'a>(
        state_stream: &mut StateStream<'a>,
        _arg: Self::Arg,
    ) -> ParseResult<'a, Self::Parsed> {
        T::parser(())
            .parse_stream(state_stream)
            .map(|value| Self::new(state_stream.state.ctx, value))
            .into_result()
    }
}

impl<T: Any + Hash + Eq + Send + StableHash> StableHash for Uniqued<T> {
    fn stable_hash(&self, ctx: &Context, state: &mut dyn Hasher) {
        // The key is an index into `ctx`'s store; hash the value it refers to.
        self.get(ctx).stable_hash(ctx, state);
    }
}

impl<T: Any + Hash + Eq + Send + CloneIntoContext> CloneIntoContext for Uniqued<T> {
    fn clone_into_context(&self, src_ctx: &Context, dst_ctx: &mut Context) -> Self {
        let value = self.get(src_ctx).clone_into_context(src_ctx, dst_ctx);
        Self::new(dst_ctx, value)
    }
}

#[cfg(test)]
mod tests {
    use crate::context::Context;
    use alloc::string::String;

    use super::{Uniqued, get, save};

    #[test]
    fn test_uniqued_any() {
        let ctx = &mut Context::new();

        let s1 = String::from("Hello");
        let s1_handle = save(ctx, s1);
        assert!(*get(ctx, s1_handle) == "Hello");

        let s2 = String::from("Hello");
        let s2_handle = save(ctx, s2);
        assert!(s1_handle == s2_handle);

        let s3 = String::from("World");
        let s3_handle = save(ctx, s3);
        assert!(s1_handle != s3_handle);

        let i1 = 71i64;
        let i1_handle = save(ctx, i1);
        assert!(*get(ctx, i1_handle) == i1);
    }

    #[test]
    fn test_uniqued_identity() {
        let ctx = &mut Context::new();

        let a = Uniqued::new(ctx, String::from("Hello"));
        let b = Uniqued::new(ctx, String::from("Hello"));
        let c = Uniqued::new(ctx, String::from("World"));
        assert_eq!(a, b);
        assert_eq!(a.key(), b.key());
        assert_ne!(a, c);
        assert_eq!(a.get(ctx), "Hello");
        assert_eq!(c.get(ctx), "World");

        // A different payload type with an equal store index is a different key.
        let n = Uniqued::new(ctx, 0u64);
        assert_eq!(*n.get(ctx), 0);
    }
}

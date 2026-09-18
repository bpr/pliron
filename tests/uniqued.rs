// SPDX-License-Identifier: Apache-2.0
// Copyright (c) The pliron contributors

//! Tests for [pliron::uniqued_any::Uniqued] as an [Attribute] field: identity
//! equality, print / parse round trip, and decontextualization.

use core::hash::{Hash, Hasher};

use pliron::{
    attribute::AttrObj,
    context::Context,
    derive::{CloneAttributeIntoContext, CloneIntoContext, StableHash, format, pliron_attr},
    irbuild::decontext::{CloneIntoContext as _, StableHash as _},
    parsable::{Parsable, parse_from_str},
    printable::Printable,
    uniqued_any::Uniqued,
    utils::table::FxHasher,
};

/// A stand-in for a canonicalized expression tree: something whose structural
/// comparison is a walk, and which is therefore worth comparing by identity.
#[derive(PartialEq, Eq, Clone, Debug, Hash, StableHash, CloneIntoContext)]
#[format("`(` $op `,` $lhs `,` $rhs `)`")]
struct Expr {
    /// An opcode: 0 add, 1 mul, 2 sub.
    op: u64,
    lhs: u64,
    rhs: u64,
}

/// An attribute holding its payload by identity.
#[pliron_attr(name = "test.uniqued_expr", format = "$0", verifier = "succ")]
#[derive(
    PartialEq, Eq, Clone, Debug, Hash, StableHash, CloneIntoContext, CloneAttributeIntoContext,
)]
struct ExprAttr(Uniqued<Expr>);

impl ExprAttr {
    fn new(ctx: &mut Context, op: u64, lhs: u64, rhs: u64) -> Self {
        ExprAttr(Uniqued::new(ctx, Expr { op, lhs, rhs }))
    }
}

fn hash_of(value: &impl Hash) -> u64 {
    let mut state = FxHasher::default();
    value.hash(&mut state);
    state.finish()
}

fn stable_hash_of(ctx: &Context, value: &AttrObj) -> u64 {
    let mut state = FxHasher::default();
    value.stable_hash(ctx, &mut state);
    state.finish()
}

#[test]
fn identity_equality() {
    let ctx = &mut Context::new();

    let a = ExprAttr::new(ctx, 0, 1, 2);
    let b = ExprAttr::new(ctx, 0, 1, 2);
    let c = ExprAttr::new(ctx, 0, 2, 1);

    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
    assert_ne!(a, c);

    // The same holds through the `Attribute` object interface.
    let a_obj: AttrObj = Box::new(a.clone());
    let b_obj: AttrObj = Box::new(b.clone());
    let c_obj: AttrObj = Box::new(c);
    assert!(a_obj.eq_attr(&*b_obj));
    let (a_hash, b_hash): (u64, u64) = (a_obj.hash_attr().into(), b_obj.hash_attr().into());
    assert_eq!(a_hash, b_hash);
    assert!(!a_obj.eq_attr(&*c_obj));

    // Identity is a handle: the payload is stored once.
    assert!(core::ptr::eq(a.0.get(ctx), b.0.get(ctx)));
    assert_eq!(a.0.get(ctx).op, 0);
}

#[test]
fn print_parse_round_trip() {
    let ctx = &mut Context::new();

    let a = ExprAttr::new(ctx, 1, 3, 4);
    let printed = a.disp(ctx).to_string();
    assert_eq!(printed, "(1,3,4)");

    // Parsing re-uniques into the same store, so the parsed attribute is
    // identical to the original, not merely structurally equal.
    let parsed = parse_from_str(ExprAttr::parser(()), ctx, &printed).expect("parse");
    assert_eq!(parsed, a);
    assert!(core::ptr::eq(parsed.0.get(ctx), a.0.get(ctx)));

    // And through the generic attribute printer and parser, which add the
    // attribute's name.
    let a_obj: AttrObj = Box::new(a);
    let printed_obj = a_obj.disp(ctx).to_string();
    let parsed_obj = parse_from_str(AttrObj::parser(()), ctx, &printed_obj).expect("parse");
    assert_eq!(parsed_obj.disp(ctx).to_string(), printed_obj);
    assert!(parsed_obj.eq_attr(&*a_obj));
}

#[test]
fn clone_into_another_context() {
    let src_ctx = &mut Context::new();
    let mut dst_ctx = Context::new();

    let a = ExprAttr::new(src_ctx, 2, 9, 8);
    let a_obj: AttrObj = Box::new(a.clone());

    // Through the attribute interface.
    let cloned = a_obj.clone_into_context(src_ctx, &mut dst_ctx);
    assert_eq!(
        a_obj.disp(src_ctx).to_string(),
        cloned.disp(&dst_ctx).to_string()
    );
    let cloned_attr = cloned
        .downcast_ref::<ExprAttr>()
        .expect("clone keeps the attribute type");
    assert_eq!(cloned_attr.0.get(&dst_ctx), a.0.get(src_ctx));

    // The stable hash is the payload's, so it agrees across contexts.
    assert_eq!(
        stable_hash_of(src_ctx, &a_obj),
        stable_hash_of(&dst_ctx, &cloned)
    );

    // Cloning re-uniques in the destination: a second clone is identical to the first.
    let cloned_again = a_obj.clone_into_context(src_ctx, &mut dst_ctx);
    assert!(cloned.eq_attr(&*cloned_again));
}

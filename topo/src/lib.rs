#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]

//! `topo` creates a hierarchy of scoped, nested [environments][crate::Env] whose shape matches the
//! function callgraph. These environments store singletons indexed by their type, and references to
//! environmental values are available only to an enclosed call scope. When a `#![topo::nested]`
//! function is called, its parent environment is cheaply propagated along with any additional
//! values added at appropriate callsites.
//!
//! Each environment in this hierarchy has a unique and deterministic [crate::Id] describing that
//! environment and the path taken to arrive at its stack frame. These identifiers are derived from
//! the path taken through the callgraph to the current location, and are stable across repeated
//! invocations of the same execution paths.
//!
//! By running the same topologically-nested functions in a loop, we can observe changes to the
//! structure over time. The [moxie](https://docs.rs/moxie) crate uses these identifiers and
//! environments to create persistent trees for rendering human interfaces.
//!
//! # Making functions nested within the call topology
//!
//! Define a topologically-nested function with the `topo::nested` attribute:
//!
//! ```
//! #[topo::nested]
//! fn basic_topo() -> topo::Id { topo::Id::current() }
//!
//! #[topo::nested]
//! fn tier_two() -> topo::Id { basic_topo() }
//!
//! // each of these functions will be run in separately identified
//! // contexts as the source locations for their calls are different
//! let first = basic_topo();
//! let second = basic_topo();
//! assert_ne!(first, second);
//!
//! let third = tier_two();
//! let fourth = tier_two();
//! assert_ne!(third, fourth);
//! assert_ne!(first, third);
//! assert_ne!(first, fourth);
//! assert_ne!(second, fourth);
//! ```
//!

#[doc(hidden)]
pub use illicit;
#[doc(inline)]
pub use topo_macro::nested;

use {
    fnv::FnvHasher,
    std::{
        any::TypeId,
        cell::RefCell,
        hash::{Hash, Hasher},
    },
};

/// Calls the provided expression with an [`Id`] specific to the callsite, optionally passing
/// additional environment values to the child scope.
///
/// ```
/// let prev = topo::Id::current();
/// topo::call(|| assert_ne!(prev, topo::Id::current()));
/// ```
///
/// Adding an `env! { ... }` directive to the macro input will take ownership of provided values
/// and make them available to the code run in the `Point` created by the invocation.
///
/// ```
/// # use topo;
/// #[derive(Debug, Eq, PartialEq)]
/// struct Submarine(usize);
///
/// assert!(topo::Env::get::<Submarine>().is_none());
///
/// topo::call(|| {
///     assert_eq!(&Submarine(1), &*topo::Env::get::<Submarine>().unwrap());
///
///     topo::call(|| {
///         assert_eq!(&Submarine(2), &*topo::Env::get::<Submarine>().unwrap());
///     }, env! {
///         Submarine => Submarine(2),
///     });
///
///     assert_eq!(&Submarine(1), &*topo::Env::get::<Submarine>().unwrap());
/// }, env! {
///     Submarine => Submarine(1),
/// });
///
/// assert!(topo::Env::get::<Submarine>().is_none());
/// ```
pub fn call<R>(op: impl FnOnce() -> R) -> R {
    unimplemented!()
}

/// todo document
pub fn call_in_slot<R>(slot: impl Hash, op: impl FnOnce() -> R) -> R {
    // $crate::unstable_raw_call!(
    //     callsite: $crate::callsite!(),
    //     slot: $slot,
    //     is_root: false,
    //     call: $($input)*
    // )
    unimplemented!()
}

fn call_inner<R>(callsite: Callsite, slot: impl Hash, op: impl FnOnce() -> R) -> R {
    unimplemented!()
}

/// Identifies an activation record in the current call topology.
///
/// The `Id` for the execution of a stack frame is the combined product of:
///
/// * a callsite: lexical source location at which the topologically-nested function was invoked
/// * parent `Id`: the identifier which was active when entering the current topo-nested function
/// * a "slot": runtime value indicating the call's "logical index" within the parent call
///
/// By default, the slot used is a count of the number of times that particular callsite has been
/// executed within the parent `Id`'s enclosing scope. This means that when creating an `Id` in a
/// loop the identifier will be unique for each "index" of the loop iteration and will be stable if
/// the same loop is invoked again. Changing the value used for the slot allows us to have stable
/// `Id`s across multiple executions when iterating over elements of a collection that itself has
/// unstable iteration order.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct Id(u64);

impl Id {
    /// Returns the `Id` for the current scope in the call topology.
    pub fn current() -> Self {
        Point::unstable_with_current(|p| p.id)
    }

    fn child(self, callsite: Callsite, slot: impl Hash) -> Self {
        let mut hasher = FnvHasher::default();
        hasher.write_u64(self.0);
        callsite.hash(&mut hasher);
        slot.hash(&mut hasher);
        Id(hasher.finish())
    }
}

impl std::fmt::Debug for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_fmt(format_args!("{:x?}", self.0))
    }
}

/// The root of a sub-graph within the overall topology formed at runtime by the call-graph of
/// topologically-nested functions.
///
/// The current `Point` contains the local [`Env`] and [`Id`].
#[doc(hiddent)]
#[derive(Debug)]
pub struct Point {
    id: Id,
    callsite: Callsite,
    /// Number of times each callsite's type has been observed during this Point.
    callsite_counts: RefCell<Vec<(Callsite, u32)>>,
}

impl Point {
    /// Mark a child Point in the topology.
    #[doc(hidden)]
    pub fn unstable_enter_child<R>(
        &self,
        callsite: Callsite,
        slot: impl Hash,
        child: impl FnOnce() -> R,
    ) -> R {
        self.increment_count(callsite);
        let child_point = Self {
            callsite,
            callsite_counts: RefCell::new(Vec::new()),
            id: self.id.child(callsite, slot),
        };

        illicit::child_env!(Point => child_point).enter(child)
    }

    /// Runs the provided closure with access to the current [`Point`].
    #[doc(hidden)]
    pub fn unstable_with_current<Out>(op: impl FnOnce(&Point) -> Out) -> Out {
        if let Some(current) = illicit::Env::get::<Self>() {
            op(&*current)
        } else {
            op(&Point::default())
        }
    }

    fn increment_count(&self, callsite: Callsite) {
        let mut counts = self.callsite_counts.borrow_mut();

        if let Some((_, count)) = counts.iter_mut().find(|(site, _)| site == &callsite) {
            *count += 1;
        } else {
            counts.push((callsite, 1));
        }
    }

    /// Returns the number of times the provided [`Callsite`] has been called within this Point.
    #[doc(hidden)]
    pub fn unstable_callsite_count(&self, callsite: Callsite) -> u32 {
        self.callsite_counts
            .borrow()
            .iter()
            .find(|(site, _)| site == &callsite)
            .map(|(_, count)| *count)
            .unwrap_or(0)
    }
}

impl Default for Point {
    fn default() -> Self {
        let callsite = unimplemented!();
        Self {
            id: Id(0),
            callsite,
            callsite_counts: Default::default(),
        }
    }
}

impl PartialEq for Point {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

/// A value unique to the source location where it is created.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Callsite {
    location: usize,
}

impl Callsite {
    #[doc(hidden)]
    pub fn new(location: &'static std::panic::Location<'static>) -> Self {
        Self {
            // the pointer value for a given location is enough to differentiate it from all others
            location: location as *const _ as usize,
        }
    }
}

/// Returns a value unique to the point of its invocation.
#[macro_export]
macro_rules! callsite {
    () => {{
        struct UwuDaddyRustcGibUniqueTypeIdPlsPls; // thanks for the great name idea, cjm00!
        $crate::Callsite::new(std::any::TypeId::of::<UwuDaddyRustcGibUniqueTypeIdPlsPls>())
    }};
}

#[cfg(test)]
mod tests {
    use {super::*, std::collections::HashSet};

    #[test]
    fn alternating_in_a_loop() {
        call(|| {
            let mut ids = HashSet::new();

            for i in 0..4 {
                if i % 2 == 0 {
                    call(|| ids.insert(Id::current()));
                } else {
                    call(|| ids.insert(Id::current()));
                }
            }

            assert_eq!(ids.len(), 4, "each callsite must produce multiple IDs");
        });
    }

    #[test]
    fn one_child_in_a_loop() {
        let root = Id::current();
        assert_eq!(
            root,
            Id::current(),
            "Id must be stable across calls within the same scope"
        );

        let mut prev = root;

        for _ in 0..100 {
            let mut called = false;
            call(|| {
                let current = Id::current();
                assert_ne!(prev, current, "each Id in this loop must be unique");
                prev = current;
                called = true;
            });

            assert_eq!(
                root,
                Id::current(),
                "Id must be stable across calls within the same scope"
            );

            let mut prev = root;

            for _ in 0..100 {
                let mut called = false;
                call(|| {
                    let current = Id::current();
                    assert_ne!(prev, current, "each Id in this loop must be unique");
                    prev = current;
                    called = true;
                });

                assert_eq!(
                    root,
                    Id::current(),
                    "outside the call must have the same Id as root"
                );
                assert!(called, "the call must be made on each loop iteration");
            }
        }
    }

    #[test]
    fn loop_over_map_with_keys_in_slots() {
        let slots = vec!["first", "second", "third", "fourth", "fifth"];

        let to_call = || {
            call(|| {
                let mut unique_ids = HashSet::new();
                for s in &slots {
                    call_in_slot(s, || {
                        let current = Id::current();
                        unique_ids.insert(current);
                    });
                }
                assert_eq!(slots.len(), unique_ids.len(), "must be one Id per slot");
                unique_ids
            })
        };

        let first = to_call();
        let second = to_call();
        assert_eq!(
            first, second,
            "same Ids must be produced for each slot each time"
        );
    }
}

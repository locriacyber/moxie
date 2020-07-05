use downcast_rs::{impl_downcast, Downcast};
use hash_hasher::HashedMap;
use parking_lot::Mutex;
use std::{
    any::{type_name, TypeId},
    borrow::Borrow,
    cell::RefCell,
    cmp::Eq,
    collections::HashMap,
    fmt::{Debug, Formatter, Result as FmtResult},
    hash::Hash,
    rc::Rc,
    sync::Arc,
};

macro_rules! doc_comment {
    ($($contents:expr)+ => $($item:tt)+) => {
        doc_comment! {@ concat!($($contents),+), $($item)+ }
    };
    (@ $contents:expr, $($item:tt)+) => {
        #[doc = $contents]
        $($item)+
    };
}

macro_rules! define_cache {
    ($name:ident $(: $bound:ident)?, $($rest:tt)*) => {
doc_comment! {"
Holds arbitrary query outputs from arbitrary inputs, namespaced by arbitrary scope types.

# Query types

> Note: the types referenced in this documentation are only visible on individual methods, as
> `" stringify!($name) "` is not itself a generic type.

Storage is sharded by the type of the query. The type of a query has three parts:
 
The query scope is the value which indexes the storage for a particular query type, it has the
bound `Scope: 'static + Eq + Hash" $(" + " stringify!($bound))? "`.

Each `Scope` corresponds to at most a single `Input: 'static" $(" + " stringify!($bound))? "`
and a single `Output: 'static" $(" + " stringify!($bound))? "` value at any given time.

# Reading stored values

See [`" stringify!($name) "::get_if_arg_eq_prev_input`] which accepts borrowed forms of `Scope`
and `Input`: `Query` and `Arg` respectively. `Arg` must satisfy `PartialEq<Input>` to determine
whether to return a stored output.

# Garbage Collection

Each time [`" stringify!($name) "::gc`] is called it acts as a barrier, removing any values
which haven't been referenced since the prior call.

After each GC, all values still in the cache are marked garbage. They are marked live again when
inserted with [`" stringify!($name) "::store`] or read with
[`" stringify!($name) "::get_if_arg_eq_prev_input`].
"=>
#[derive(Debug, Default)]
pub struct $name {
    /// We use a [`hash_hasher::HashedMap`] here because we know that `Query` is made up only of
    /// `TypeIds` which come pre-hashed courtesy of rustc.
    inner: HashedMap<Query, Box<dyn Gc $(+ $bound)?>>,
}

impl $name {
    /// Return a reference to a query's stored output if a result is stored and `arg` equals the
    /// previously-stored `Input`. If a reference is returned, the stored input/output
    /// is marked live and will not be GC'd the next call.
    pub fn get_if_arg_eq_prev_input<Query, Scope, Arg, Input, Output>(
        &mut self,
        query: &Query,
        arg: &Arg,
    ) -> Option<&Output>
    where
        Query: Eq + Hash + ToOwned<Owned = Scope> + ?Sized,
        Scope: 'static + Borrow<Query> + Eq + Hash $(+ $bound)?,
        Arg: PartialEq<Input> + ?Sized,
        Input: 'static + Borrow<Arg> $(+ $bound)?,
        Output: 'static $(+ $bound)?,
    {
        self.get_namespace_mut::<Scope, Input, Output>().get_if_input_eq(query, arg)
    }

    /// Stores the input/output of a query which will not be GC'd at the next call.
    pub fn store<Query, Scope, Input, Output>(
        &mut self,
        query: &Query,
        input: Input,
        output: Output,
    ) where
        Query: Eq + Hash + ToOwned<Owned = Scope> + ?Sized,
        Scope: 'static + Borrow<Query> + Eq + Hash $(+ $bound)?,
        Input: 'static $(+ $bound)?,
        Output: 'static $(+ $bound)?,
    {
        self.get_namespace_mut().store(query, input, output);
    }

    fn get_namespace_mut<Scope, Input, Output>(&mut self) -> &mut Namespace<Scope, Input, Output>
    where
        Scope: 'static + Eq + Hash $(+ $bound)?,
        Input: 'static $(+ $bound)?,
        Output: 'static $(+ $bound)?,
    {
        let gc: &mut (dyn Gc $(+ $bound)?) = &mut **self
            .inner
            .entry(Query::get::<Scope, Input, Output>())
            .or_insert_with(|| Box::new(Namespace::<Scope, Input, Output>::default()));
        gc.as_any_mut().downcast_mut().unwrap()
    }

    /// Drops cached values that were not referenced since the last call
    /// and sets all remaining values to be dropped by default in the next call.
    pub fn gc(&mut self) {
        for namespace in self.inner.values_mut() {
            namespace.gc();
        }
    }
}

impl std::panic::UnwindSafe for $name {}
impl std::panic::RefUnwindSafe for $name {}

paste::item! {
    define_cache! {
        @handle $name $(: $bound)?, [<Shared $name>], $($rest)*
    }
}
    };
    (
        @handle
        $name:ident $(: $bound:ident)?,
        $handle:ident,
        $shared:ident,
        $lock:ident :: $acquire:ident
    ) => {

/// Provides access to a shared cache which stores results from arbitrary queries
/// for later retrieval.
#[derive(Clone)]
pub struct $handle {
    inner: $shared<$lock<$name>>,
}

impl Default for $handle {
    fn default() -> Self {
        Self {
            inner: $shared::new($lock::new($name::default()))
        }
    }
}

impl $handle {
    /// Provides a closure-based caching API on top of the underlying
    /// mutable API. Steps:
    ///
    /// 1. if matching cached value, mark live and return
    /// 2. no cached value, initialize new one
    /// 3. store new value as live, return
    ///
    /// Both (1) and (3) require mutable access to storage. We want to allow
    /// nested cached `init`s eventually so it's important that (2) *doesn't*
    /// use mutable access to storage.
    pub fn cache_with<Query, Scope, Arg, Input, Output, Ret>(
        &self,
        query: &Query,
        arg: &Arg,
        init: impl FnOnce(&Input) -> Output,
        with: impl FnOnce(&Output) -> Ret,
    ) -> Ret
    where
        Query: Eq + Hash + ToOwned<Owned = Scope> + ?Sized,
        Scope: 'static + Borrow<Query> + Eq + Hash $(+ $bound)?,
        Arg: PartialEq<Input> + ToOwned<Owned=Input> + ?Sized,
        Input: 'static + Borrow<Arg> $(+ $bound)?,
        Output: 'static $(+ $bound)?,
        Ret: 'static $(+ $bound)?,
    {
        if let Some(stored) = { self.inner.$acquire().get_if_arg_eq_prev_input(query, arg) } {
            return with(stored);
        }

        let arg = arg.to_owned();
        let to_store = init(&arg);
        let to_return = with(&to_store);
        self.inner.$acquire().store(query, arg, to_store);
        to_return
    }

    /// See `gc` on the inner cache type.
    pub fn gc(&self) {
        self.inner.$acquire().gc()
    }
}

impl Debug for $handle {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        f.debug_tuple(stringify!($handle))
            .field(&*self.inner.$acquire())
            .finish()
    }
}

impl From<$name> for $handle {
    fn from(inner: $name) -> Self {
        Self { inner: $shared::new($lock::new(inner)) }
    }
}
    };
}

define_cache!(LocalCache, Rc, RefCell::borrow_mut);
define_cache!(Cache: Send, Arc, Mutex::lock);

struct Namespace<Scope, Input, Output> {
    inner: HashMap<Scope, (Liveness, Input, Output)>,
}

impl<Scope, Input, Output> Namespace<Scope, Input, Output>
where
    Scope: Eq + Hash + 'static,
    Input: 'static,
    Output: 'static,
{
    fn get_if_input_eq<Query, Arg>(&mut self, query: &Query, input: &Arg) -> Option<&Output>
    where
        Query: Eq + Hash + ?Sized,
        Scope: Borrow<Query>,
        Arg: PartialEq<Input> + ?Sized,
        Input: Borrow<Arg>,
    {
        let (ref mut liveness, ref stored_input, ref stored) = self.inner.get_mut(query)?;
        if input == stored_input {
            *liveness = Liveness::Live;
            Some(stored)
        } else {
            None
        }
    }

    fn store<Query>(&mut self, query: &Query, input: Input, output: Output)
    where
        Query: Eq + Hash + ToOwned<Owned = Scope> + ?Sized,
        Scope: Borrow<Query>,
    {
        if let Some((liveness, prev_input, prev_output)) = self.inner.get_mut(query) {
            *liveness = Liveness::Live;
            *prev_input = input;
            *prev_output = output;
        } else {
            let scope = query.to_owned();
            self.inner.insert(scope, (Liveness::Live, input, output));
        }
    }
}

impl<Scope, Input, Output> Gc for Namespace<Scope, Input, Output>
where
    Scope: Eq + Hash + 'static,
    Input: 'static,
    Output: 'static,
{
    fn gc(&mut self) {
        self.inner.retain(|_, (l, _, _)| *l == Liveness::Live);
        self.inner.values_mut().for_each(|(l, _, _)| *l = Liveness::Dead);
    }
}

impl<Scope, Input, Output> Default for Namespace<Scope, Input, Output>
where
    Scope: Eq + Hash + 'static,
    Input: 'static,
    Output: 'static,
{
    fn default() -> Self {
        Self { inner: Default::default() }
    }
}

impl<Scope, Input, Output> Debug for Namespace<Scope, Input, Output>
where
    Scope: Eq + Hash + 'static,
    Input: 'static,
    Output: 'static,
{
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        f.debug_map()
            .entry(&"scope", &type_name::<Scope>())
            .entry(&"input", &type_name::<Input>())
            .entry(&"output", &type_name::<Output>())
            .finish()
    }
}

/// Each query has an `Input`, and an `Output` which together can be
/// thought of as defining a function: `(input) -> output`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
struct Query {
    /// The type of scope by which the query is namespaced.
    scope: TypeId,
    /// The type of input the query accepts.
    input: TypeId,
    /// The type of output the query returns.
    output: TypeId,
}

impl Query {
    fn get<Scope, Input, Output>() -> Self
    where
        Scope: 'static,
        Input: 'static,
        Output: 'static,
    {
        Self {
            scope: TypeId::of::<Scope>(),
            input: TypeId::of::<Input>(),
            output: TypeId::of::<Output>(),
        }
    }
}

/// A type which can contain values of varying liveness.
trait Gc: Downcast + Debug {
    /// Remove dead entries.
    fn gc(&mut self);
}

impl_downcast!(Gc);

/// Describes the outcome for a cached value if a garbage collection were
/// to occur when observed. During the run of a `Revision` any cached values
/// which are initialized or read are marked as `Live`. At the end of a
/// `Revision`,
#[derive(Debug, PartialEq)]
enum Liveness {
    /// The value would be retained in a GC right now.
    Live,
    /// The value would be dropped in a GC right now.
    Dead,
}

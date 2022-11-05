use std::any::Any;
use std::fmt::{Debug, Display};
use std::hash::Hash;

use super::Handler;
use crate::{Actor, Scheduler};

/// Information for constructing re-actor thread pools
pub struct Pool<R: Actor, L: Layout> {
    pub(super) id: L,
    pub(super) scheduler: Box<dyn Scheduler<R>>,
    pub(super) handler: Box<dyn Handler<L>>,
}

impl<R: Actor, L: Layout> Pool<R, L> {
    /// Constructs pool information from the source data
    pub fn new(
        id: L,
        scheduler: impl Scheduler<R> + 'static,
        handler: impl Handler<L> + 'static,
    ) -> Self {
        Pool {
            id,
            scheduler: Box::new(scheduler),
            handler: Box::new(handler),
        }
    }
}

/// Trait layout out the structure for the re-actor runtime.
/// It is should be implemented by enumerators listing existing thread pools.
pub trait Layout: Send + Copy + Eq + Hash + Debug + Display + From<u32> + Into<u32> {
    /// The root actor type which will be operated by the re-actor
    type RootActor: Actor<Layout = Self>;

    /// Provides re-actor with information on constructing thread pools.
    fn default_pools() -> Vec<Pool<Self::RootActor, Self>>;

    /// Convertor transforming any context from the actor hierarchy into the
    /// root actor context.
    fn convert(other_ctx: Box<dyn Any>) -> <Self::RootActor as Actor>::Context;
}

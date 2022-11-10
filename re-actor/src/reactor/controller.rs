use std::collections::HashMap;

use crossbeam_channel as chan;

use super::runtime::ControlEvent;
use crate::{Actor, InternalError, Layout, Reactor};

/// API for controlling the [`Reactor`] by the re-actor instance or through
/// multiple [`Controller`]s constructed by [`Reactor::controller`].
pub trait ReactorApi {
    /// Resource type managed by the re-actor.
    type Actor: Actor;

    /// Enumerator for specific re-actor runtimes
    type Pool: Layout<RootActor = Self::Actor>;

    /// Connects new resource and adds it to the manager.
    fn start_actor(
        &mut self,
        pool: Self::Pool,
        ctx: <Self::Actor as Actor>::Context,
    ) -> Result<(), InternalError<Self::Pool>>;

    /// Disconnects from a resource, providing a reason.
    fn stop_actor(
        &mut self,
        id: <Self::Actor as Actor>::Id,
    ) -> Result<(), InternalError<Self::Pool>>;

    /// Set one-time timer which will call [`Handler::on_timer`] upon expiration.
    fn set_timer(&mut self, pool: Self::Pool) -> Result<(), InternalError<Self::Pool>>;

    /// Send data to the resource.
    fn send(
        &mut self,
        id: <Self::Actor as Actor>::Id,
        cmd: <Self::Actor as Actor>::Cmd,
    ) -> Result<(), InternalError<Self::Pool>>;
}

/// Instance of re-actor controller which may be transferred between threads
pub struct Controller<L: Layout> {
    actor_map: HashMap<<L::RootActor as Actor>::Id, L>,
    channels: HashMap<L, chan::Sender<ControlEvent<L::RootActor>>>,
}

impl<L: Layout> Clone for Controller<L> {
    fn clone(&self) -> Self {
        Controller {
            actor_map: self.actor_map.clone(),
            channels: self.channels.clone(),
        }
    }
}

impl<L: Layout> Controller<L> {
    pub(super) fn new() -> Self {
        Controller {
            actor_map: empty!(),
            channels: empty!(),
        }
    }

    pub(super) fn channel_for(
        &self,
        pool: L,
    ) -> Result<&chan::Sender<ControlEvent<L::RootActor>>, InternalError<L>> {
        self.channels
            .get(&pool)
            .ok_or(InternalError::UnknownPool(pool))
    }

    /// Returns in which pool an actor is run in.
    pub fn pool_for(&self, id: <L::RootActor as Actor>::Id) -> Result<L, InternalError<L>> {
        self.actor_map
            .get(&id)
            .ok_or(InternalError::UnknownActor(id))
            .copied()
    }

    fn register_actor(
        &mut self,
        id: <L::RootActor as Actor>::Id,
        pool: L,
    ) -> Result<(), InternalError<L>> {
        if !self.channels.contains_key(&pool) {
            return Err(InternalError::UnknownPool(pool));
        }
        if self.actor_map.contains_key(&id) {
            return Err(InternalError::RepeatedActor(id));
        }
        self.actor_map.insert(id, pool);
        Ok(())
    }

    pub(super) fn register_pool(
        &mut self,
        pool: L,
        channel: chan::Sender<ControlEvent<L::RootActor>>,
    ) -> Result<(), InternalError<L>> {
        if self.channels.contains_key(&pool) {
            return Err(InternalError::RepeatedPoll(pool));
        }
        self.channels.insert(pool, channel);
        Ok(())
    }
}

impl<L: Layout> ReactorApi for Controller<L> {
    type Actor = L::RootActor;
    type Pool = L;

    fn start_actor(
        &mut self,
        pool: L,
        ctx: <Self::Actor as Actor>::Context,
    ) -> Result<(), InternalError<L>> {
        self.channel_for(pool)?.send(ControlEvent::Connect(ctx))?;
        Ok(())
    }

    fn stop_actor(&mut self, id: <Self::Actor as Actor>::Id) -> Result<(), InternalError<L>> {
        let pool = self.pool_for(id.clone())?;
        self.channel_for(pool)?.send(ControlEvent::Disconnect(id))?;
        Ok(())
    }

    fn set_timer(&mut self, pool: L) -> Result<(), InternalError<L>> {
        self.channel_for(pool)?.send(ControlEvent::SetTimer())?;
        Ok(())
    }

    fn send(
        &mut self,
        id: <Self::Actor as Actor>::Id,
        cmd: <Self::Actor as Actor>::Cmd,
    ) -> Result<(), InternalError<L>> {
        let pool = self.pool_for(id.clone())?;
        self.channel_for(pool)?.send(ControlEvent::Send(id, cmd))?;
        Ok(())
    }
}

impl<L: Layout> ReactorApi for Reactor<L> {
    type Actor = L::RootActor;
    type Pool = L;

    fn start_actor(
        &mut self,
        pool: L,
        ctx: <Self::Actor as Actor>::Context,
    ) -> Result<(), InternalError<L>> {
        self.controller.start_actor(pool, ctx)
    }

    fn stop_actor(&mut self, id: <Self::Actor as Actor>::Id) -> Result<(), InternalError<L>> {
        self.controller.stop_actor(id)
    }

    fn set_timer(&mut self, pool: L) -> Result<(), InternalError<L>> {
        self.controller.set_timer(pool)
    }

    fn send(
        &mut self,
        id: <Self::Actor as Actor>::Id,
        cmd: <Self::Actor as Actor>::Cmd,
    ) -> Result<(), InternalError<L>> {
        self.controller.send(id, cmd)
    }
}

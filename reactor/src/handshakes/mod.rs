use std::collections::VecDeque;
use streampipes::{Resource, ResourceAddr};

pub struct Event;

pub struct Events<A: ResourceAddr>(VecDeque<(A::Addr, Event)>);

impl<A: ResourceAddr> Interator for Events<A> {}

pub struct Popol<R: Resource> {
    resources: Vec<R>,
    pub events: Events<R::Addr>,
}

impl<R: Resource> Popol<R> {
    pub fn poll(&mut self) -> &Events<R::Addr> {

    }
}

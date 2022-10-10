pub struct Runtime<H: nakamoto_net::Protocol> {
    handler: H
}

impl<H: nakamoto_net::Protocol> Runtime<H> {
    pub fn run(&self) -> ! {
        loop {

        }
    }
}

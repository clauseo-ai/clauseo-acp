//! The pipe's instances, with no Windows call (`windows_pipe.rs` gives the real [`Pipes`], the
//! tests a fake): one listening, the rest connected clients.
//!
//! **The invariant: the last instance is never closed.** A pipe name lives as long as one of its
//! instances. Were the count to reach zero, the name would be free, and a pool user spinning on
//! `CreateNamedPipeW` with the published name could make the next instance, with a descriptor
//! of their own, and read every refusal sent to it. So:
//!
//! - a listener whose client connected leaves the listening slot only once its replacement is
//!   made (or the instances are at [`MAX_INSTANCES`], when the count stays the same);
//! - a listener whose connect failed is closed only once its replacement is made;
//! - each poll listens again (when there is room) before it prunes the clients that left, so a
//!   client that connects and leaves within one poll is closed with a listener already open;
//! - a client that left, or whose write failed, is closed only when another instance is left;
//!   before closing the last one, a listener is made;
//! - a later instance must be owned by SYSTEM (checked on its handle), or the run ends;
//! - a later instance that can't be made, or a count that reaches zero all the same, ends the
//!   run: the caller deletes the published name and exits 74, and the next run starts under a
//!   new random name with `FILE_FLAG_FIRST_PIPE_INSTANCE`.

use super::Failed;

/// The most instances: clients, and one listening while there is room.
pub const MAX_INSTANCES: usize = 8;

/// The pipe's instances as the bookkeeping sees them.
pub trait Pipes {
    /// One instance.
    type Pipe;
    /// A later instance (never the first), its connect pending.
    fn create(&mut self) -> Result<Self::Pipe, Failed>;
    /// Whether `pipe`'s owner is LOCAL SYSTEM, read from its handle.
    fn owned_by_system(&mut self, pipe: &Self::Pipe) -> Result<bool, Failed>;
    /// Whether `pipe`'s client has connected: `None` when the connect failed.
    fn accepted(&mut self, pipe: &mut Self::Pipe) -> Option<bool>;
    /// Whether `pipe`'s client is still there. `Err` when that can't be told (the handle
    /// lacks the access to ask), which ends the run rather than drop every client unseen.
    fn alive(&mut self, pipe: &Self::Pipe) -> Result<bool, Failed>;
    /// Writes all of `line` to `pipe`'s client: whether it went.
    fn write(&mut self, pipe: &Self::Pipe, line: &[u8]) -> bool;
    /// Disconnects and closes `pipe`.
    fn close(&mut self, pipe: Self::Pipe);
}

/// The instances: at most one listening, the rest clients; never none.
pub struct Instances<P: Pipes> {
    pipes: P,
    listener: Option<P::Pipe>,
    clients: Vec<P::Pipe>,
}

impl<P: Pipes> Instances<P> {
    /// The instances, from the `first`, made with `FILE_FLAG_FIRST_PIPE_INSTANCE`.
    pub fn new(pipes: P, first: P::Pipe) -> Instances<P> {
        Instances {
            pipes,
            listener: Some(first),
            clients: Vec::new(),
        }
    }

    /// How many instances are held.
    pub fn count(&self) -> usize {
        usize::from(self.listener.is_some()) + self.clients.len()
    }

    /// How many clients are connected.
    pub fn clients(&self) -> usize {
        self.clients.len()
    }

    /// The pipes, for the tests' fake.
    pub fn pipes(&mut self) -> &mut P {
        &mut self.pipes
    }

    /// Takes a client that connected, drops clients that left, listens again while there is
    /// room, and gives how many clients are connected. `Err` ends the run (74).
    pub fn poll(&mut self) -> Result<usize, Failed> {
        if let Some(mut listener) = self.listener.take() {
            match self.pipes.accepted(&mut listener) {
                Some(false) => self.listener = Some(listener),
                Some(true) => {
                    // The replacement first, when there is room for both.
                    if self.clients.len() + 2 <= MAX_INSTANCES {
                        match self.later() {
                            Ok(next) => self.listener = Some(next),
                            Err(why) => {
                                self.clients.push(listener);
                                return Err(why);
                            }
                        }
                    }
                    self.clients.push(listener);
                }
                None => {
                    // A failed connect: its instance goes once another is there.
                    if self.clients.len() + 2 <= MAX_INSTANCES {
                        match self.later() {
                            Ok(next) => self.listener = Some(next),
                            Err(why) => {
                                self.listener = Some(listener);
                                return Err(why);
                            }
                        }
                    }
                    self.pipes.close(listener);
                }
            }
        }
        // Listen again before pruning, so a client that came and went in one poll is closed
        // with another instance already open.
        self.listen_if_room()?;
        let mut gone = Vec::new();
        let mut failed = None;
        for client in std::mem::take(&mut self.clients) {
            match self.pipes.alive(&client) {
                Ok(false) if failed.is_none() => gone.push(client),
                Ok(_) => self.clients.push(client),
                Err(why) => {
                    failed.get_or_insert(why);
                    self.clients.push(client);
                }
            }
        }
        if let Some(why) = failed {
            // Kept, to close after the name is deleted.
            self.clients.extend(gone);
            return Err(why);
        }
        self.retire(gone)?;
        self.listen_if_room()?;
        self.checked()?;
        Ok(self.clients.len())
    }

    /// A listener, when there is none and room for one.
    fn listen_if_room(&mut self) -> Result<(), Failed> {
        if self.listener.is_none() && self.count() < MAX_INSTANCES {
            self.listener = Some(self.later()?);
        }
        Ok(())
    }

    /// Writes `line` to every client; one whose write fails is dropped. `Err` ends the run.
    pub fn send(&mut self, line: &[u8]) -> Result<(), Failed> {
        let mut gone = Vec::new();
        for client in std::mem::take(&mut self.clients) {
            if self.pipes.write(&client, line) {
                self.clients.push(client);
            } else {
                gone.push(client);
            }
        }
        self.retire(gone)?;
        self.checked()
    }

    /// Closes every instance: only once the published name is deleted.
    pub fn close_all(&mut self) {
        for pipe in self
            .listener
            .take()
            .into_iter()
            .chain(self.clients.drain(..))
        {
            self.pipes.close(pipe);
        }
    }

    /// A later instance, owned by SYSTEM, or the reason the run ends.
    fn later(&mut self) -> Result<P::Pipe, Failed> {
        let pipe = self.pipes.create()?;
        match self.pipes.owned_by_system(&pipe) {
            Ok(true) => Ok(pipe),
            Ok(false) => {
                self.pipes.close(pipe);
                Err("a later instance of the pipe isn't owned by SYSTEM".to_owned())
            }
            Err(why) => {
                self.pipes.close(pipe);
                Err(why)
            }
        }
    }

    /// Closes `gone`, already out of the clients. Before the last instance of all closes, a
    /// listener is made, so the name always has one. On `Err` the rest are kept, to close after
    /// the name is deleted.
    fn retire(&mut self, mut gone: Vec<P::Pipe>) -> Result<(), Failed> {
        while let Some(pipe) = gone.pop() {
            if self.count() == 0 && gone.is_empty() {
                match self.later() {
                    Ok(next) => self.listener = Some(next),
                    Err(why) => {
                        self.clients.push(pipe);
                        return Err(why);
                    }
                }
            }
            self.pipes.close(pipe);
        }
        Ok(())
    }

    /// `Err` when no instance is held, which the invariant rules out.
    fn checked(&self) -> Result<(), Failed> {
        if self.count() == 0 {
            return Err("the pipe has no instance left".to_owned());
        }
        Ok(())
    }
}

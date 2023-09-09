use super::{AnySocket, Error, Result, Socket, SocketRef, SocketType};
use atat::atat_derive::AtatLen;
use heapless::Vec;
use serde::{Deserialize, Serialize};

/// A handle, identifying a socket in a set.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    AtatLen,
    Ord,
    hash32_derive::Hash32,
    Default,
    Serialize,
    Deserialize,
)]
pub struct Handle(pub u8);

/// An extensible set of sockets.
#[derive(Default, Debug)]
pub struct Set<const N: usize, const L: usize> {
    pub sockets: Vec<Option<Socket<L>>, N>,
}

impl<const N: usize, const L: usize> Set<N, L> {
    /// Create a socket set using the provided storage.
    pub fn new() -> Set<N, L> {
        let mut sockets = Vec::new();
        while sockets.len() < N {
            sockets.push(None).ok();
        }
        Set { sockets }
    }

    /// Get the maximum number of sockets the set can hold
    pub fn capacity(&self) -> usize {
        N
    }

    /// Get the current number of initialized sockets, the set is holding
    pub fn len(&self) -> usize {
        self.sockets.iter().filter(|a| a.is_some()).count()
    }

    /// Check if the set is currently holding no active sockets
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the type of a specific socket in the set.
    ///
    /// Returned as a [`SocketType`]
    pub fn socket_type(&self, handle: Handle) -> Option<SocketType> {
        if let Ok(index) = self.index_of(handle) {
            if let Some(socket) = self.sockets.get(index) {
                return socket.as_ref().map(|s| s.get_type());
            }
        }
        None
    }

    /// Add a socket to the set with the reference count 1, and return its handle.
    pub fn add<T>(&mut self, socket: T) -> Result<Handle>
    where
        T: Into<Socket<L>>,
    {
        let socket = socket.into();
        let handle = socket.handle();

        debug!(
            "[Socket Set] Adding: {} {:?} to: {:?}",
            handle.0,
            socket.get_type(),
            self
        );

        if self.index_of(handle).is_ok() {
            return Err(Error::DuplicateSocket);
        }

        self.sockets
            .iter_mut()
            .find(|s| s.is_none())
            .ok_or(Error::SocketSetFull)?
            .replace(socket);

        Ok(handle)
    }

    /// Get a socket from the set by its handle, as mutable.
    pub fn get<T: AnySocket<L>>(&mut self, handle: Handle) -> Result<SocketRef<T>> {
        let index = self.index_of(handle)?;

        match self.sockets.get_mut(index).ok_or(Error::InvalidSocket)? {
            Some(socket) => Ok(T::downcast(SocketRef::new(socket))?),
            None => Err(Error::InvalidSocket),
        }
    }

    /// Get the index of a given socket in the set.
    fn index_of(&self, handle: Handle) -> Result<usize> {
        self.sockets
            .iter()
            .position(|i| {
                i.as_ref()
                    .map(|s| s.handle().0 == handle.0)
                    .unwrap_or(false)
            })
            .ok_or(Error::InvalidSocket)
    }

    /// Remove a socket from the set
    pub fn remove(&mut self, handle: Handle) -> Result<()> {
        let index = self.index_of(handle)?;
        let item: &mut Option<Socket<L>> =
            self.sockets.get_mut(index).ok_or(Error::InvalidSocket)?;

        debug!(
            "[Socket Set] Removing socket! {} {:?}",
            handle.0,
            item.as_ref().map(|i| i.get_type())
        );

        item.take().ok_or(Error::InvalidSocket)?;
        Ok(())
    }

    /// Prune the sockets in this set.
    ///
    /// All sockets are removed and dropped.
    pub fn prune(&mut self) {
        debug!("[Socket Set] Pruning: {:?}", self);
        self.sockets.iter_mut().enumerate().for_each(|(_, slot)| {
            slot.take();
        })
    }

    pub fn recycle(&mut self) -> bool {
        let h = self.iter().find(|(_, s)| s.recycle()).map(|(h, _)| h);
        if h.is_none() {
            return false;
        }
        self.remove(h.unwrap()).is_ok()
    }

    /// Iterate every socket in this set.
    pub fn iter(&self) -> impl Iterator<Item = (Handle, &Socket<L>)> {
        self.sockets.iter().filter_map(|slot| {
            if let Some(socket) = slot {
                Some((Handle(socket.handle().0), socket))
            } else {
                None
            }
        })
    }

    /// Iterate every socket in this set, as SocketRef.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Handle, SocketRef<Socket<L>>)> {
        self.sockets.iter_mut().filter_map(|slot| {
            if let Some(socket) = slot {
                Some((Handle(socket.handle().0), SocketRef::new(socket)))
            } else {
                None
            }
        })
    }
}

#[cfg(feature = "defmt")]
impl<const N: usize, const L: usize> defmt::Format for Set<N, L> {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "[");
        for socket in self.iter() {
            match socket.1 {
                Socket::Udp(s) => defmt::write!(fmt, "[{:?}, UDP({:?})],", socket.0, s.state()),
                Socket::Tcp(s) => defmt::write!(fmt, "[{:?}, TCP({:?})],", socket.0, s.state()),
            }
        }
        defmt::write!(fmt, "]");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TcpSocket, UdpSocket};

    #[test]
    fn add_socket() {
        let mut set = Set::<2, 64>::new();

        assert_eq!(set.add(TcpSocket::new(0)), Ok(Handle(0)));
        assert_eq!(set.len(), 1);
        assert_eq!(set.add(UdpSocket::new(1)), Ok(Handle(1)));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn remove_socket() {
        let mut set = Set::<2, 64>::new();

        assert_eq!(set.add(TcpSocket::new(0)), Ok(Handle(0)));
        assert_eq!(set.len(), 1);
        assert_eq!(set.add(UdpSocket::new(1)), Ok(Handle(1)));
        assert_eq!(set.len(), 2);

        assert!(set.remove(Handle(0)).is_ok());
        assert_eq!(set.len(), 1);

        assert!(set.get::<TcpSocket<64>>(Handle(0)).is_err());

        set.get::<UdpSocket<64>>(Handle(1))
            .expect("failed to get udp socket");
    }

    #[test]
    fn add_duplicate_socket() {
        let mut set = Set::<2, 64>::new();

        assert_eq!(set.add(TcpSocket::new(0)), Ok(Handle(0)));
        assert_eq!(set.len(), 1);
        assert_eq!(set.add(UdpSocket::new(0)), Err(Error::DuplicateSocket));
    }

    #[test]
    fn add_socket_to_full_set() {
        let mut set = Set::<2, 64>::new();

        assert_eq!(set.add(TcpSocket::new(0)), Ok(Handle(0)));
        assert_eq!(set.len(), 1);
        assert_eq!(set.add(UdpSocket::new(1)), Ok(Handle(1)));
        assert_eq!(set.len(), 2);
        assert_eq!(set.add(UdpSocket::new(2)), Err(Error::SocketSetFull));
    }

    #[test]
    fn get_socket() {
        let mut set = Set::<2, 64>::new();

        assert_eq!(set.add(TcpSocket::new(0)), Ok(Handle(0)));
        assert_eq!(set.len(), 1);
        assert_eq!(set.add(UdpSocket::new(1)), Ok(Handle(1)));
        assert_eq!(set.len(), 2);

        set.get::<TcpSocket<64>>(Handle(0))
            .expect("failed to get tcp socket");

        set.get::<UdpSocket<64>>(Handle(1))
            .expect("failed to get udp socket");
    }

    #[test]
    fn get_socket_wrong_type() {
        let mut set = Set::<2, 64>::new();

        assert_eq!(set.add(TcpSocket::new(0)), Ok(Handle(0)));
        assert_eq!(set.len(), 1);
        assert_eq!(set.add(UdpSocket::new(1)), Ok(Handle(1)));
        assert_eq!(set.len(), 2);

        assert!(set.get::<TcpSocket<64>>(Handle(1)).is_err());

        set.get::<UdpSocket<64>>(Handle(1))
            .expect("failed to get udp socket");
    }

    #[test]
    fn get_socket_type() {
        let mut set = Set::<2, 64>::new();

        assert_eq!(set.add(TcpSocket::new(0)), Ok(Handle(0)));
        assert_eq!(set.len(), 1);
        assert_eq!(set.add(UdpSocket::new(1)), Ok(Handle(1)));
        assert_eq!(set.len(), 2);

        assert_eq!(set.socket_type(Handle(0)), Some(SocketType::Tcp));
        assert_eq!(set.socket_type(Handle(1)), Some(SocketType::Udp));
    }

    #[test]
    fn replace_socket() {
        let mut set = Set::<2, 64>::new();

        assert_eq!(set.add(TcpSocket::new(0)), Ok(Handle(0)));
        assert_eq!(set.len(), 1);
        assert_eq!(set.add(UdpSocket::new(1)), Ok(Handle(1)));
        assert_eq!(set.len(), 2);

        assert!(set.remove(Handle(0)).is_ok());
        assert_eq!(set.len(), 1);

        assert!(set.get::<TcpSocket<64>>(Handle(0)).is_err());

        set.get::<UdpSocket<64>>(Handle(1))
            .expect("failed to get udp socket");

        assert_eq!(set.add(TcpSocket::new(0)), Ok(Handle(0)));
        assert_eq!(set.len(), 2);

        set.get::<TcpSocket<64>>(Handle(0))
            .expect("failed to get tcp socket");
    }

    #[test]
    fn prune_socket_set() {
        let mut set = Set::<2, 64>::new();

        assert_eq!(set.add(TcpSocket::new(0)), Ok(Handle(0)));
        assert_eq!(set.len(), 1);
        assert_eq!(set.add(UdpSocket::new(1)), Ok(Handle(1)));
        assert_eq!(set.len(), 2);

        set.get::<TcpSocket<64>>(Handle(0))
            .expect("failed to get tcp socket");

        set.prune();
        assert_eq!(set.len(), 0);
    }
}

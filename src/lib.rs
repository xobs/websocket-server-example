pub mod api;

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

/// Representation of a websocket file descriptor
#[derive(Eq, Hash, PartialEq)]
struct WebSocketFd(u16);

struct WebSocketReceiver {
    pipe: mpsc::Sender<WebSocketPacket>,
}

/// A packet that has been received from the Xous Websocket Server
pub struct WebSocketPacket {
    backing: xous::MemoryRange,
    valid: usize,
}

impl WebSocketPacket {
    pub fn new(backing: xous::MemoryRange, valid: Option<xous::MemorySize>) -> WebSocketPacket {
        // Make sure `valid` doesn't exceed the size of the buffer.
        let valid = backing
            .len()
            .min(valid.map(|v| v.get()).unwrap_or_default());
        WebSocketPacket { backing, valid }
    }

    pub fn len(&self) -> usize {
        self.valid
    }

    /// Return a slice over the internal buffer. The slice will be of a type that you specify,
    /// for example:
    ///     let x: &[u8] = packet1.as_slice();
    pub fn as_slice<T>(&self) -> &[T] {
        unsafe {
            core::slice::from_raw_parts(
                self.backing.as_ptr() as *const T,
                self.len() / core::mem::size_of::<T>(),
            )
        }
    }

    pub fn as_slice_mut<T>(&mut self) -> &mut [T] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.backing.as_mut_ptr() as *mut T,
                self.len() / core::mem::size_of::<T>(),
            )
        }
    }
}

/// Unmap the backing store when this packet goes out of scope.
impl Drop for WebSocketPacket {
    fn drop(&mut self) {
        xous::unmap_memory(self.backing).expect("couldn't free memory");
    }
}

/// A thread that lives inside a process to poll websocket connections. It calls `Poll` on
/// the websocket server and passes it a buffer. When data is available, this buffer will
/// be returned filled with data. The amount of data that is available will be in the `valid`
/// slot.
fn websocket_poll_thread(
    websocket_server_cid: xous::CID,
    receivers: Arc<Mutex<HashMap<WebSocketFd, WebSocketReceiver>>>,
) {
    loop {
        // Allocate a new buffer to pass to this thread. This memory is managed by us, and
        // will need to be freed with `xous::unmap_memory(buffer)`.
        let buffer = xous::map_memory(
            None,
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("out of memory");

        // Create the `Poll` message with no additional arguments (i.e. `Offset` and `Valid` set to None)
        let msg = xous::Message::new_lend_mut(api::Opcodes::Poll as usize, buffer, None, None);

        // Send the `Poll` message to the websocket_cid. Because this is a `lend_mut`, this will
        // block forever until the server shuts down or responds.
        let response = xous::send_message(websocket_server_cid, msg).expect("couldn't send");

        // When memory is returned, there are two `usize` of information that are attached
        // to the response. These are currently called `offset` and `valid`, however they
        // are actually arbitrary information and may be reused as user values. I really want
        // to change this inside libxous, since it's completely not obvious.
        if let xous::Result::MemoryReturned(offset, valid) = response {
            // `offset` is an `Option<NonZeroUSize>`, so turn it into a normal `usize`.
            // One of the many warts that I would like to fix in a v2 of the library.
            let target_fd = WebSocketFd(offset.map(|o| o.get()).unwrap_or(0).try_into().unwrap());

            // Send the websocket to the Channel that's waiting to receive it. This will transfer ownership
            // of the data there, so it's up to that thread to free the message.
            if let Some(receiver) = receivers.lock().unwrap().get(&target_fd) {
                if let Ok(()) = receiver.pipe.send(WebSocketPacket::new(buffer, valid)) {
                    // The message was successfully transferred, loop around.
                    continue;
                }
            } else {
                println!("Error: got a message for a WebSocketFd that doesn't exist!");
            }

            // There was an error sending the message. Free the buffer and try again.
            xous::unmap_memory(buffer).expect("couldn't free memory");
        }
    }
}

#[derive(Clone)]
pub struct WebSocketService {
    receivers: Arc<Mutex<HashMap<WebSocketFd, WebSocketReceiver>>>,
    cid: xous::CID,
}

impl WebSocketService {
    pub fn new() -> WebSocketService {
        let receivers = Arc::new(Mutex::new(HashMap::new()));
        // This should be replaced with a call to `xous-names`, instead of using a hardcoded server name.
        let cid = xous::connect(xous::SID::from_bytes(b"~xous-websocket~").unwrap())
            .expect("couldn't connect to websocket server");
        {
            let receivers = receivers.clone();
            std::thread::spawn(move || websocket_poll_thread(cid, receivers));
        }
        WebSocketService { receivers, cid }
    }
}

pub struct WebSocketStream {
    fd: WebSocketFd,
}
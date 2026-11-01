//! Persistent WebSocket transport. Only reconnect and liveness have timers;
//! project updates and selection commands are sent immediately.
use eframe::egui;
use serde_json::Value;
use std::sync::mpsc::{self, Receiver};

pub(crate) struct Socket {
    pub events: Receiver<Result<Value, String>>,
    #[cfg(not(target_arch = "wasm32"))]
    sender: mpsc::Sender<Value>,
    #[cfg(target_arch = "wasm32")]
    handle: eframe::wasm_bindgen::JsValue,
    #[cfg(target_arch = "wasm32")]
    _wake: eframe::wasm_bindgen::closure::Closure<dyn FnMut()>,
}
/// Tickets are accepted only on the configured server's host, over TLS except
/// localhost. They are short-lived, single-use and never put in a URL.
pub(crate) fn valid_endpoint(server: &str, endpoint: &str) -> bool {
    fn authority(url: &str) -> Option<(&str, &str)> {
        let (scheme, rest) = url.split_once("://")?;
        let authority = rest.split('/').next()?;
        if authority.contains(['@', '?', '#', '\\']) || authority.is_empty() {
            return None;
        }
        let host = if authority.starts_with('[') {
            authority.split(']').next()?.strip_prefix('[')?
        } else {
            authority.split(':').next()?
        };
        Some((scheme, host))
    }
    if endpoint.contains(['\r', '\n', '?', '#']) {
        return false;
    }
    let Some((source, host)) = authority(server) else {
        return false;
    };
    let Some((scheme, target)) = authority(endpoint) else {
        return false;
    };
    host.eq_ignore_ascii_case(target)
        && (scheme == "wss"
            || (scheme == "ws"
                && source == "http"
                && matches!(host, "localhost" | "127.0.0.1" | "::1")))
}
impl Socket {
    pub fn connect(ctx: &egui::Context, url: String, ticket: String, connection: String) -> Self {
        let (sender, events) = mpsc::channel();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (outgoing, commands) = mpsc::channel::<Value>();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = (|| -> Result<(), String> {
                    use std::{
                        io::ErrorKind,
                        net::{TcpStream, ToSocketAddrs},
                        time::Duration,
                    };
                    use tungstenite::{Message, client::IntoClientRequest, stream::MaybeTlsStream};
                    let _ = rustls::crypto::ring::default_provider().install_default();
                    let request = url.into_client_request().map_err(|e| e.to_string())?;
                    let host = request.uri().host().ok_or("WebSocket host missing")?;
                    let port = request.uri().port_u16().unwrap_or(
                        if request.uri().scheme_str() == Some("wss") {
                            443
                        } else {
                            80
                        },
                    );
                    let mut stream = None;
                    for address in (host, port).to_socket_addrs().map_err(|e| e.to_string())? {
                        if let Ok(socket) =
                            TcpStream::connect_timeout(&address, Duration::from_secs(5))
                        {
                            stream = Some(socket);
                            break;
                        }
                    }
                    let stream = stream.ok_or("WebSocket connection failed")?;
                    stream
                        .set_read_timeout(Some(Duration::from_secs(10)))
                        .map_err(|e| e.to_string())?;
                    stream
                        .set_write_timeout(Some(Duration::from_secs(10)))
                        .map_err(|e| e.to_string())?;
                    let (mut socket, _) =
                        tungstenite::client_tls(request, stream).map_err(|e| e.to_string())?;
                    match socket.get_mut() {
                        MaybeTlsStream::Plain(s) => {
                            s.set_read_timeout(Some(Duration::from_millis(20)))
                        }
                        MaybeTlsStream::Rustls(s) => {
                            s.sock.set_read_timeout(Some(Duration::from_millis(20)))
                        }
                        _ => return Err("Unsupported WebSocket TLS stream".into()),
                    }
                    .map_err(|e| e.to_string())?;
                    socket.send(Message::Text(serde_json::json!({"type":"authenticate","ticket":ticket,"connection":connection}).to_string().into())).map_err(|e|e.to_string())?;
                    loop {
                        loop {
                            match commands.try_recv() {
                                Ok(command) => socket
                                    .send(Message::Text(command.to_string().into()))
                                    .map_err(|e| e.to_string())?,
                                Err(mpsc::TryRecvError::Empty) => break,
                                Err(mpsc::TryRecvError::Disconnected) => {
                                    let _ = socket.close(None);
                                    return Ok(());
                                }
                            }
                        }
                        match socket.read() {
                            Ok(Message::Text(text)) => {
                                let value =
                                    serde_json::from_str(&text).map_err(|e| e.to_string())?;
                                if sender.send(Ok(value)).is_err() {
                                    return Ok(());
                                }
                                ctx.request_repaint();
                            }
                            Ok(Message::Close(_)) => return Err("WebSocket disconnected".into()),
                            Ok(_) => {
                                let _ = socket.flush();
                            }
                            Err(tungstenite::Error::Io(e))
                                if matches!(
                                    e.kind(),
                                    ErrorKind::WouldBlock | ErrorKind::TimedOut
                                ) => {}
                            Err(e) => return Err(format!("WebSocket disconnected: {e}")),
                        }
                    }
                })();
                if let Err(error) = result {
                    let _ = sender.send(Err(error));
                    ctx.request_repaint();
                }
            });
            Self {
                events,
                sender: outgoing,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            use eframe::wasm_bindgen::{JsCast, JsValue, closure::Closure};
            let queue = js_sys::Array::new();
            let ctx = ctx.clone();
            let queue_copy = queue.clone();
            let wake = Closure::wrap(Box::new(move || {
                while queue_copy.length() > 0 {
                    let value = queue_copy.shift();
                    let result = value
                        .as_string()
                        .ok_or_else(|| "Invalid WebSocket event".to_owned())
                        .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()));
                    let result = result.and_then(|v| {
                        if v["type"] == "transport_error" {
                            Err("WebSocket disconnected".into())
                        } else {
                            Ok(v)
                        }
                    });
                    let _ = sender.send(result);
                }
                ctx.request_repaint();
            }) as Box<dyn FnMut()>);
            let create = js_sys::Function::new_with_args(
                "url,ticket,connection,queue,wake",
                r#"
                const socket=new WebSocket(url);
                const emit=v=>{queue.push(JSON.stringify(v));wake();};
                socket.onopen=()=>socket.send(JSON.stringify({type:'authenticate',ticket,connection}));
                socket.onmessage=e=>{try{if(e.data.length>17825792)throw Error();emit(JSON.parse(e.data));}catch{socket.close();emit({type:'transport_error'});}};
                socket.onerror=()=>emit({type:'transport_error'});
                socket.onclose=()=>emit({type:'transport_error'});
                return socket;
            "#,
            );
            let args = js_sys::Array::new();
            args.push(&JsValue::from_str(&url));
            args.push(&JsValue::from_str(&ticket));
            args.push(&JsValue::from_str(&connection));
            args.push(&queue);
            args.push(wake.as_ref());
            let handle = create.apply(&JsValue::NULL, &args).unwrap_or(JsValue::NULL);
            let _ = wake.as_ref().dyn_ref::<js_sys::Function>();
            Self {
                events,
                handle,
                _wake: wake,
            }
        }
    }
    pub fn send(&self, message: Value) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.sender
                .send(message)
                .map_err(|_| "WebSocket disconnected".into())
        }
        #[cfg(target_arch = "wasm32")]
        {
            use eframe::wasm_bindgen::JsCast;
            let method = js_sys::Reflect::get(&self.handle, &"send".into())
                .map_err(|_| "WebSocket unavailable")?
                .dyn_into::<js_sys::Function>()
                .map_err(|_| "WebSocket unavailable")?;
            method
                .call1(&self.handle, &message.to_string().into())
                .map(|_| ())
                .map_err(|_| "WebSocket disconnected".into())
        }
    }
}
#[cfg(target_arch = "wasm32")]
impl Drop for Socket {
    fn drop(&mut self) {
        // Remove JS callbacks before dropping their Rust closure.
        js_sys::Function::new_with_args(
            "s",
            "if(s){s.onopen=s.onmessage=s.onerror=s.onclose=null;s.close();}",
        )
        .call1(&eframe::wasm_bindgen::JsValue::NULL, &self.handle)
        .ok();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ticket_destinations_require_same_host_and_secure_transport() {
        assert!(valid_endpoint(
            "https://example.com",
            "wss://example.com/ws"
        ));
        assert!(valid_endpoint(
            "http://localhost:8081",
            "ws://localhost:8082/ws"
        ));
        for bad in [
            "ws://example.com/ws",
            "wss://evil.example/ws",
            "wss://user@example.com/ws",
            "wss://example.com/ws?ticket=secret",
        ] {
            assert!(!valid_endpoint("https://example.com", bad));
        }
    }
}

use ureq::{
    Agent, Error,
    config::Config,
    http::Uri,
    unversioned::{
        resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
        transport::{
            Buffers, ConnectionDetails, Connector, DefaultConnector, LazyBuffers, NextTimeout,
            Transport,
        },
    },
};

#[derive(Debug)]
struct ProxyResolver {
    uri: Uri,
}
impl Resolver for ProxyResolver {
    fn resolve(
        &self,
        _uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> std::result::Result<ResolvedSocketAddrs, Error> {
        DefaultResolver::default().resolve(&self.uri, config, timeout)
    }
}
#[derive(Debug)]
struct ForwardConnector {
    proxy: Uri,
}
impl Connector for ForwardConnector {
    type Out = ForwardTransport;
    fn connect(
        &self,
        d: &ConnectionDetails<'_>,
        _: Option<()>,
    ) -> std::result::Result<Option<Self::Out>, Error> {
        let details = ConnectionDetails {
            uri: &self.proxy,
            addrs: d.addrs.clone(),
            config: d.config,
            request_level: d.request_level,
            resolver: d.resolver,
            now: d.now,
            timeout: d.timeout,
            current_time: d.current_time.clone(),
            run_connector: d.run_connector.clone(),
            register_shutdown: d.register_shutdown.clone(),
        };
        Ok(DefaultConnector::default()
            .connect(&details, None)?
            .map(|inner| ForwardTransport {
                inner,
                buffers: LazyBuffers::new(
                    d.config.input_buffer_size(),
                    d.config.output_buffer_size(),
                ),
                target: d.uri.to_string(),
                first: true,
            }))
    }
}
#[derive(Debug)]
struct ForwardTransport {
    inner: Box<dyn Transport>,
    buffers: LazyBuffers,
    target: String,
    first: bool,
}
impl Transport for ForwardTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }
    fn transmit_output(
        &mut self,
        amount: usize,
        timeout: NextTimeout,
    ) -> std::result::Result<(), Error> {
        let bytes = &self.buffers.output()[..amount];
        let output = if self.first {
            self.first = false;
            let start = bytes
                .iter()
                .position(|&b| b == b' ')
                .ok_or(Error::ConnectionFailed)?;
            let end = bytes[start + 1..]
                .iter()
                .position(|&b| b == b' ')
                .map(|i| i + start + 1)
                .ok_or(Error::ConnectionFailed)?;
            [
                bytes[..start + 1].to_vec(),
                self.target.as_bytes().to_vec(),
                bytes[end..].to_vec(),
            ]
            .concat()
        } else {
            bytes.to_vec()
        };
        let mut sent = 0;
        while sent < output.len() {
            let buffer = self.inner.buffers().output();
            let n = buffer.len().min(output.len() - sent);
            buffer[..n].copy_from_slice(&output[sent..sent + n]);
            self.inner.transmit_output(n, timeout)?;
            sent += n;
        }
        Ok(())
    }
    fn await_input(&mut self, timeout: NextTimeout) -> std::result::Result<bool, Error> {
        if !self.inner.maybe_await_input(timeout)? {
            return Ok(false);
        }
        let input = self.inner.buffers().input();
        let buffer = self.buffers.input_append_buf();
        let n = input.len().min(buffer.len());
        buffer[..n].copy_from_slice(&input[..n]);
        self.buffers.input_appended(n);
        self.inner.buffers().input_consume(n);
        Ok(n > 0)
    }
    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }
}
pub(super) fn agent(config: Config, proxy: &ureq::Proxy) -> Agent {
    let uri = proxy.uri().clone();
    Agent::with_parts(
        config,
        ForwardConnector { proxy: uri.clone() },
        ProxyResolver { uri },
    )
}

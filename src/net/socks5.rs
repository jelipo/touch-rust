use std::borrow::Borrow;
use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;

use async_std::future;
use async_std::future::*;
use async_std::io;
use async_std::io::{Error, ErrorKind};
use async_std::io::ReadExt;
use async_std::net::{Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use async_std::prelude::*;
use async_std::task::JoinHandle;
use async_trait::async_trait;
use log::{error, info, trace, warn};

use crate::core::profile::BasePassiveConfig;
use crate::net::proxy::{InputProxy, OutputProxy, ProxyInfo, ProxyReader, ProxyWriter};
use crate::socks::consts::Socks5Header;
use crate::socks::socks5_connector::Socks5Connector;

pub struct Socks5Passive {
    tcp_listerner: TcpListener,
    password: Option<String>,
    out_proxy: Box<dyn OutputProxy>,
}

impl Socks5Passive {
    /// Init Socks5 Passive. And try to bind host and port
    pub async fn new(passive: &BasePassiveConfig, out_proxy: Box<dyn OutputProxy>) -> io::Result<Self> {
        let addr_str = format!("{}:{}", &passive.local_host, passive.local_port);
        let addr = SocketAddr::from_str(addr_str.as_str()).or(
            Err(Error::new(ErrorKind::InvalidInput, "Error address"))
        );
        let tcp_listener = TcpListener::bind(addr?).await?;
        info!("Socks5 bind in {}", addr_str);
        Ok(Self {
            tcp_listerner: tcp_listener,
            password: passive.password.clone(),
            out_proxy,
        })
    }
}


#[async_trait(? Send)]
impl InputProxy for Socks5Passive {
    async fn start(&mut self) -> io::Result<()> {
        loop {
            let mut tcpstream: TcpStream = self.tcp_listerner.incoming().next().await.ok_or(
                io::Error::new(ErrorKind::InvalidInput, "")
            )??;
            let mut connector = Socks5Connector::new(&mut tcpstream);
            let proxy_info = connector.check().await?;
            if let Err(e) = new_proxy(&mut self.out_proxy, &mut tcpstream, proxy_info).await {
                error!("Socks5 proxy error. {}", e)
            };
        }
    }
}


async fn new_proxy(out_proxy: &mut Box<dyn OutputProxy>, input_stream: &mut TcpStream, info: ProxyInfo) -> io::Result<()> {
    let (mut out_reader, mut out_writer) =
        out_proxy.new_connect(info).await;
    let mut input_read = input_stream.clone();
    let mut input_write = input_stream.clone();


    let read = async {
        read(input_read, out_writer).await
    };
    let write = async {
        write(input_write, out_reader).await
    };
    let result = read.race(write).await;

    Ok(())
}


async fn read(mut input_read: TcpStream, mut out_writer: Box<dyn ProxyWriter + Send>) -> io::Result<()> {
    let mut buf = [0u8; 1024];
    loop {
        let size = input_read.read(&mut buf).await?;
        if size == 0 { break; }
        out_writer.write(&buf[0..size]).await?;
    }
    Ok(())
}

async fn write(mut input_write: TcpStream, mut out_reader: Box<dyn ProxyReader + Send>) -> io::Result<()> {
    loop {
        let data = out_reader.read().await?;
        if data.len() == 0 { break; }
        input_write.write_all(data.as_slice()).await?;
    }
    Ok(())
}
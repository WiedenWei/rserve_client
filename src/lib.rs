use bytes::{Buf, BufMut, Bytes};
use tokio;
use tokio::io::AsyncWriteExt;

pub enum RserveConnection {
    Tcp(tokio::net::TcpStream),
    Unix(tokio::net::UnixStream),
}

pub enum ReturnValue {
    Char(char),
    Int(i32),
    Double(f64),
    Null(String),
    Bool(bool),
    Str(String),

    IntVec(Vec<i32>),
    DoubleVec(Vec<f64>),
    BoolVec(Vec<bool>),
    StrVec(Vec<String>),
}

impl RserveConnection {
    async fn readable(&mut self) -> std::io::Result<()> {
        match self {
            RserveConnection::Tcp(stream) => stream.readable().await?,
            RserveConnection::Unix(stream) => stream.readable().await?,
        }
        Ok(())
    }

    fn try_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            RserveConnection::Tcp(stream) => stream.try_read(buf),
            RserveConnection::Unix(stream) => stream.try_read(buf),
        }
    }

    async fn writable(&self) -> std::io::Result<()> {
        match self {
            RserveConnection::Tcp(stream) => stream.writable().await?,
            RserveConnection::Unix(stream) => stream.writable().await?,
        }
        Ok(())
    }
    fn try_write(&self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            RserveConnection::Tcp(stream) => stream.try_write(buf),
            RserveConnection::Unix(stream) => stream.try_write(buf),
        }
    }
    async fn shut_down(&mut self) -> std::io::Result<()> {
        match self {
            RserveConnection::Tcp(stream) => stream.shutdown().await?,
            RserveConnection::Unix(stream) => stream.shutdown().await?,
        }
        Ok(())
    }

    // Implement other methods of RserveConnection similarly
    // command: null terminated c-string, for example "1+1\0"
    // if you got a error code, please run self.eval("geterrmessage()", false).await?
    // to get R error information
    pub async fn eval(
        &mut self,
        command: &str,
        void: bool,
    ) -> Result<ReturnValue, Box<dyn std::error::Error>> {
        // write
        self.writable().await?;
        let cmd = Bytes::from(command.to_string());
        let cmd_length = cmd.len() as i32;

        let mut message_header = vec![];
        if void {
            message_header.put_i32_le(0x002_i32); // CMD_VOID_EVAL
        } else {
            message_header.put_i32_le(0x003_i32); // CMD_EVAL
        }
        message_header.put_i32_le(cmd_length + 4);
        message_header.put_i32_le(0_i32);
        message_header.put_i32_le(0_i32);

        let mut data_header = vec![];
        data_header.put_u8(0x04_u8);
        data_header.put_i32_le(cmd_length);

        let mut message = vec![];
        message.put(&message_header[..]);
        message.put(&data_header[..4]);
        message.put(&cmd[..]);

        match self.try_write(&message) {
            Ok(n) => {
                assert_eq!(n, message.len());
            }
            Err(ref e) if e.kind() == tokio::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                return Err(e.into());
            }
        };

        // read response
        loop {
            self.readable().await?;
            let mut data = vec![0_u8; 1024];
            match self.try_read(&mut data) {
                Ok(n) => {
                    let mut res_data = &data[..n];
                    // response message header 16 bytes
                    let cmd_res = res_data.get_i32_le(); // 0-3
                    let err_code = (cmd_res >> 24) & 127;
                    let response_code = cmd_res & 0xfffff;

                    //error eval, return error info
                    if response_code != (0x10000 | 0x0001) {
                        /*
                          use async_recursion::async_recursion
                          let err_info = self.eval(stream, "geterrmessage()", false).await?;
                        */
                        let err_info = format!("error code: {}", err_code);
                        return Err(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            err_info,
                        )));
                    }

                    /*
                    ignore message header remain field

                    let data_length = res_data.get_i32_le();//4-7
                    let data_offset = res_data.get_i32_le();//8-11, 0
                    let data_header_length2 = res_data.get_i32_le(); //12-15
                    */
                    res_data.advance(12);

                    // response message data header 4 bytes
                    let data_type = res_data.get_u8(); //16
                    //let raw_data_header_length2 = res_data.take(3);//17-19
                    res_data.advance(3);
                    //let mut dst = vec![];
                    //dst.put(raw_data_header_length2);
                    //dst.put_u8(0_u8);
                    //let data_length2 = (&dst[..]).get_i32_le();

                    match data_type {
                        // DT_INT
                        1_u8 => {
                            let a = res_data.get_i32_le();
                            return Ok(ReturnValue::Int(a));
                        }
                        // DT_CHAR
                        2_u8 => {
                            let a = res_data.get_u8() as char;
                            return Ok(ReturnValue::Char(a));
                        }
                        // DT_DOUBLE
                        3_u8 => {
                            let a = res_data.get_f64_le();
                            return Ok(ReturnValue::Double(a));
                        }
                        // DT_STRING 0 terminted string
                        4_u8 => {
                            let a = res_data.chunk().to_vec();
                            let s = String::from_utf8(a).unwrap();
                            return Ok(ReturnValue::Str(s));
                        }

                        // DT_SEXP
                        10_u8 => {
                            let expression_type = res_data.get_u8(); // eXpression Type
                            let raw_data_header_length2 = res_data.take(3); // (24-bit int) length
                            let mut dst = vec![];
                            dst.put(raw_data_header_length2);
                            dst.put_u8(0_u8);
                            let data_length2 = (&dst[..]).get_i32_le();
                            // pass header part
                            res_data.advance(3);

                            match expression_type {
                                // XT_NULL
                                0_u8 => {
                                    return Ok(ReturnValue::Null("NULL".to_string()));
                                }
                                // XT_INT
                                1_u8 => {
                                    let a = res_data.get_i32_le();
                                    return Ok(ReturnValue::Int(a));
                                }
                                // XT_DOUBLE
                                2_u8 => {
                                    let a = res_data.get_f64_le();
                                    return Ok(ReturnValue::Double(a));
                                }
                                // XT_STR
                                3_u8 => {
                                    let a = res_data.chunk().to_vec();
                                    let s = String::from_utf8(a).unwrap();
                                    return Ok(ReturnValue::Str(s));
                                }
                                // XT_BOOL
                                6_u8 => {
                                    let a = res_data.get_u8();
                                    if a == 1 {
                                        return Ok(ReturnValue::Bool(true));
                                    } else {
                                        return Ok(ReturnValue::Bool(true));
                                    }
                                }
                                // XT_ARRAY_INT
                                32_u8 => {
                                    let mut a: Vec<i32> = vec![];
                                    for _ in 0..data_length2 / 4 {
                                        a.push(res_data.get_i32_le());
                                    }
                                    return Ok(ReturnValue::IntVec(a));
                                }
                                // XT_ARRAY_DOUBLE
                                33_u8 => {
                                    let mut a: Vec<f64> = vec![];
                                    for _ in 0..data_length2 / 8 {
                                        a.push(res_data.get_f64_le());
                                    }
                                    return Ok(ReturnValue::DoubleVec(a));
                                }
                                // XT_ARRAY_STR
                                34_u8 => {
                                    let a: Vec<String> = String::from_utf8(
                                        res_data.take(data_length2 as usize).chunk().to_vec(),
                                    )
                                    .unwrap()
                                    .split("\0")
                                    .map(|word| word.to_string())
                                    .collect();
                                    return Ok(ReturnValue::StrVec(a));
                                }
                                // XT_ARRAY_BOOL
                                36_u8 => {
                                    let mut a: Vec<bool> = vec![];
                                    for _ in 0..data_length2 {
                                        let b = res_data.get_u8();
                                        if b == 1 {
                                            a.push(true);
                                        } else {
                                            a.push(false);
                                        }
                                    }
                                    return Ok(ReturnValue::BoolVec(a));
                                }

                                _ => {
                                    return Err(Box::new(std::io::Error::new(
                                        std::io::ErrorKind::Unsupported,
                                        "unsupported outcome type!",
                                    )));
                                }
                            }
                        }
                        // DT_BYTE
                        // DT_ARRAY
                        // DT_CUSTOM
                        // DT_LARGE
                        _ => {
                            return Err(Box::new(std::io::Error::new(
                                std::io::ErrorKind::Unsupported,
                                "unsupported outcome type!",
                            )));
                        }
                    };
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    return Err(e.into());
                }
            };
        }
    }
}

pub async fn connect(addr: &str) -> Result<RserveConnection, Box<dyn std::error::Error>> {
    if addr.starts_with("tcp://") {
        let addr = addr.trim_start_matches("tcp://");
        let s = tokio::net::TcpStream::connect(addr).await?;
        loop {
            s.readable().await?;
            let mut data = vec![0_u8; 1024];
            match s.try_read(&mut data) {
                Ok(n) => {
                    let string_result = String::from_utf8_lossy(&data[..n]);
                    assert!(string_result.starts_with("Rsrv01"));
                    break;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        Ok(RserveConnection::Tcp(s))
    } else if addr.starts_with("unix://") {
        let path = addr.trim_start_matches("unix://");
        let ss = tokio::net::UnixStream::connect(path).await?;
        loop {
            ss.readable().await?;
            let mut data = vec![0_u8; 1024];
            match ss.try_read(&mut data) {
                Ok(n) => {
                    let string_result = String::from_utf8_lossy(&data[..n]);
                    assert!(string_result.starts_with("Rsrv01"));
                    break;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        Ok(RserveConnection::Unix(ss))
    } else {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid address format",
        )))
    }
}

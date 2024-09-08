# Rserve_client
Rserve_client is a simple asynchronous Rust client for Rserve (https://www.rforge.net/Rserve/).

# Motivation
Rust has its own advatages on compution effeciency and memory safety, while R is good at data science, expecially for visualization, statistics and probability.
It is wonduful that combined them together, Rserve_rs allow R code execution from Rust through unix docmain socket and tcp scoket.

# Core
1. connection type and connect function
```
pub enum RserveConnection {
    Tcp(tokio::net::TcpStream),
    Unix(tokio::net::UnixStream),
}

pub async fn connect(addr: &str) -> Result<RserveConnection, Box<dyn std::error::Error>>{...}
```
2. eval return type and eval function
```
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
    async fn eval(
        &mut self,
        command: &str,
        void: bool,
    ) -> Result<ReturnValue, Box<dyn std::error::Error>> {...}
}
```

# Example
```
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = connect("unix:///home/deng/Rserve_working_directory/rserve.sock").await?;
    match a.eval("1+1\0", false).await {
        Ok(out) => match out {
        ReturnValue::DoubleVec(c) => println!("{}", c[0]),
        ReturnValue::StrVec(c) => print!("{}", c[0]),
        _ => println!("{}", "other data type!"),
        },
        Err(e) => {
            println!("{}", e);
            let cerr = a.eval("geterrmessage()", false).await?;
            match cerr {
                ReturnValue::StrVec(a) => println!("{}", a[0]),
                _ => println!("{}", "other data type!"),
            }
        }
    }
    a.shut_down().await?;
    Ok(())
}
```
output: 2.0

# NOTE
This Rust crate is under early deveploment, thus the API may change in near furture. It currently support 1024 byte length of eval R code and return value.

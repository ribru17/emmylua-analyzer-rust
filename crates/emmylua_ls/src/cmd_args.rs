use structopt::StructOpt;

#[allow(unused)]
#[derive(Debug, StructOpt, Clone)]
#[structopt(name = "emmylua-ls", about = "EmmyLua Language Server")]
pub struct CmdArgs {
    /// Communication method
    #[structopt(
        long = "communication",
        short = "c",
        help = "Communication method",
        default_value = "stdio"
    )]
    pub communication: Communication,

    /// IP address to listen on (only valid when using TCP)
    #[structopt(
        long = "ip",
        help = "IP address to listen on",
        default_value = "127.0.0.1"
    )]
    pub ip: String,

    /// Port number to listen on (only valid when using TCP)
    #[structopt(
        long = "port",
        help = "Port number to listen on",
        default_value = "5007"
    )]
    pub port: u16,

    /// Logging level (e.g., "error", "warn", "info", "debug", "trace")
    #[structopt(long = "log-level", help = "Logging level", default_value = "info")]
    pub log_level: LogLevel,

    /// Path to the log file
    #[structopt(
        long = "log-path",
        help = "Path to the log file. Use 'none' to disable log file output.",
        default_value = ""
    )]
    pub log_path: NoneableString,

    /// Path to the resources and logs directory. Use 'none' to indicate that assets should not be output to the file system.
    #[structopt(
        long = "resources-path",
        help = "Path to the resources. Use 'none' to disable resources file output.",
        default_value = ""
    )]
    pub resources_path: NoneableString,

    /// Whether to load the standard library.
    #[structopt(
        long = "load-stdlib",
        help = "Whether to load the standard library",
        default_value = "true"
    )]
    pub load_std_lib: CmdBool,
}

/// Logging level enum
#[derive(Debug, StructOpt, Clone, Copy)]
#[structopt(rename_all = "lowercase")]
pub enum LogLevel {
    /// Error level
    Error,
    /// Warning level
    Warn,
    /// Info level
    Info,
    /// Debug level
    Debug,
}

impl std::str::FromStr for LogLevel {
    type Err = String;

    fn from_str(input: &str) -> Result<LogLevel, Self::Err> {
        match input.to_lowercase().as_str() {
            "error" => Ok(LogLevel::Error),
            "warn" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            _ => Err(format!(
                "Invalid log level: '{}'. Please choose 'error', 'warn', 'info', 'debug'",
                input
            )),
        }
    }
}

#[derive(Debug, StructOpt, Clone, Copy)]
#[structopt(rename_all = "lowercase")]
pub enum Communication {
    Stdio,
    Tcp,
}

impl std::str::FromStr for Communication {
    type Err = String;

    fn from_str(input: &str) -> Result<Communication, Self::Err> {
        match input.to_lowercase().as_str() {
            "stdio" => Ok(Communication::Stdio),
            "tcp" => Ok(Communication::Tcp),
            _ => Err(format!(
                "Invalid communication method: '{}'. Please choose 'stdio', 'tcp'",
                input
            )),
        }
    }
}

/// A string that can be "None" to represent an empty option
#[derive(Debug, Clone)]
pub struct NoneableString(pub Option<String>);

impl std::str::FromStr for NoneableString {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("none") {
            Ok(NoneableString(None))
        } else {
            Ok(NoneableString(Some(s.to_string())))
        }
    }
}

#[allow(unused)]
impl NoneableString {
    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

#[derive(Debug, Clone)]
pub struct CmdBool(pub bool);

impl std::str::FromStr for CmdBool {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "true" => Ok(CmdBool(true)),
            "false" => Ok(CmdBool(false)),
            _ => Err(format!(
                "Invalid boolean value: '{}'. Please choose 'true' or 'false'",
                s
            )),
        }
    }
}

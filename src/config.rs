//! Dependency-free parsing of Riscbox configuration files.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

pub const CONFIG_VERSION: i32 = 1;
pub const MAX_DRIVES: usize = 4;
pub const MAX_FILESYSTEMS: usize = 4;
pub const MAX_NETWORK_INTERFACES: usize = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i32),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    #[must_use]
    pub fn as_integer(&self) -> Option<i32> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl Error for ParseError {}

/// Parses the relaxed JSON syntax used by deployed Riscbox configuration files.
///
/// # Errors
///
/// Returns the byte offset and reason when the input is malformed.
pub fn parse_value(source: &str) -> Result<Value, ParseError> {
    let mut parser = Parser::new(source);
    let value = parser.value()?;
    parser.skip_space_and_comments()?;
    if parser.peek().is_some() {
        return Err(parser.error("unexpected characters after value"));
    }
    Ok(value)
}

struct Parser<'a> {
    source: &'a [u8],
    offset: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            offset: 0,
        }
    }

    fn error(&self, message: &str) -> ParseError {
        ParseError {
            offset: self.offset,
            message: message.to_owned(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.offset).copied()
    }

    fn take(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.offset += 1;
        Some(byte)
    }

    fn skip_space_and_comments(&mut self) -> Result<(), ParseError> {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.offset += 1;
            }
            if self.source.get(self.offset..self.offset + 2) == Some(b"//") {
                self.offset += 2;
                while self.peek().is_some_and(|byte| byte != b'\n') {
                    self.offset += 1;
                }
            } else if self.source.get(self.offset..self.offset + 2) == Some(b"/*") {
                self.offset += 2;
                let Some(end) = self.source[self.offset..]
                    .windows(2)
                    .position(|window| window == b"*/")
                else {
                    return Err(self.error("unterminated comment"));
                };
                self.offset += end + 2;
            } else {
                return Ok(());
            }
        }
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        self.skip_space_and_comments()?;
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Value::String),
            Some(byte) if byte.is_ascii_digit() => self.integer(),
            Some(byte) if identifier_start(byte) => {
                let ident = self.identifier()?;
                match ident.as_str() {
                    "null" => Ok(Value::Null),
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    _ => Err(self.error("unknown identifier")),
                }
            }
            Some(_) => Err(self.error("unexpected character")),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn object(&mut self) -> Result<Value, ParseError> {
        self.take();
        let mut values = BTreeMap::new();
        loop {
            self.skip_space_and_comments()?;
            if self.peek() == Some(b'}') {
                self.take();
                return Ok(Value::Object(values));
            }
            let name = match self.peek() {
                Some(b'"') => self.string()?,
                Some(byte) if identifier_start(byte) => self.identifier()?,
                _ => return Err(self.error("invalid property name")),
            };
            if name.is_empty() {
                return Err(self.error("invalid property name"));
            }
            self.skip_space_and_comments()?;
            if self.take() != Some(b':') {
                return Err(self.error("expected ':'"));
            }
            values.insert(name, self.value()?);
            self.skip_space_and_comments()?;
            match self.peek() {
                Some(b',') => {
                    self.take();
                }
                Some(b'}') => {}
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Value, ParseError> {
        self.take();
        let mut values = Vec::new();
        loop {
            self.skip_space_and_comments()?;
            if self.peek() == Some(b']') {
                self.take();
                return Ok(Value::Array(values));
            }
            values.push(self.value()?);
            self.skip_space_and_comments()?;
            match self.peek() {
                Some(b',') => {
                    self.take();
                }
                Some(b']') => {}
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.take();
        let mut bytes = Vec::new();
        loop {
            match self.take() {
                Some(b'"') => {
                    return String::from_utf8(bytes)
                        .map_err(|_| self.error("string is not valid UTF-8"));
                }
                Some(b'\n' | b'\r') | None => return Err(self.error("unterminated string")),
                Some(b'\\') => {
                    let escaped = match self.take() {
                        Some(b'\'' | b'"' | b'\\') => self.source[self.offset - 1],
                        Some(b'n') => b'\n',
                        Some(b'r') => b'\r',
                        Some(b't') => b'\t',
                        Some(b'x') => {
                            let high = self.hex_digit()?;
                            let low = self.hex_digit()?;
                            (high << 4) | low
                        }
                        _ => return Err(self.error("unknown escape code")),
                    };
                    bytes.push(escaped);
                }
                Some(byte) => bytes.push(byte),
            }
        }
    }

    fn hex_digit(&mut self) -> Result<u8, ParseError> {
        match self.take() {
            Some(byte @ b'0'..=b'9') => Ok(byte - b'0'),
            Some(byte @ b'a'..=b'f') => Ok(byte - b'a' + 10),
            Some(byte @ b'A'..=b'F') => Ok(byte - b'A' + 10),
            _ => Err(self.error("invalid hex digit")),
        }
    }

    fn identifier(&mut self) -> Result<String, ParseError> {
        let start = self.offset;
        self.offset += 1;
        while self
            .peek()
            .is_some_and(|byte| identifier_start(byte) || byte.is_ascii_digit())
        {
            self.offset += 1;
        }
        String::from_utf8(self.source[start..self.offset].to_vec())
            .map_err(|_| self.error("invalid identifier"))
    }

    fn integer(&mut self) -> Result<Value, ParseError> {
        let start = self.offset;
        let radix = if self.source.get(self.offset..self.offset + 2) == Some(b"0x")
            || self.source.get(self.offset..self.offset + 2) == Some(b"0X")
        {
            self.offset += 2;
            16
        } else if self.peek() == Some(b'0') {
            self.offset += 1;
            8
        } else {
            10
        };
        let digits = self.offset;
        while self.peek().is_some_and(|byte| match radix {
            16 => byte.is_ascii_hexdigit(),
            10 => byte.is_ascii_digit(),
            _ => matches!(byte, b'0'..=b'7'),
        }) {
            self.offset += 1;
        }
        if radix == 8 && self.offset == digits {
            return Ok(Value::Integer(0));
        }
        if self.offset == digits {
            return Err(self.error("integer has no digits"));
        }
        let text = std::str::from_utf8(&self.source[digits..self.offset])
            .map_err(|_| self.error("invalid integer"))?;
        let value =
            i32::from_str_radix(text, radix).map_err(|_| self.error("integer out of range"))?;
        if self.offset == start {
            return Err(self.error("invalid integer"));
        }
        Ok(Value::Integer(value))
    }
}

fn identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$')
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Console {
    #[default]
    Virtio,
    Uart,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriveConfig {
    pub file: String,
    pub device: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FilesystemBackend {
    File(String),
    Socket(String),
    JavaScript9p,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilesystemConfig {
    pub backend: FilesystemBackend,
    pub tag: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkConfig {
    pub driver: String,
    pub interface_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayConfig {
    pub device: String,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VmConfig {
    pub machine: String,
    pub memory_size_mib: i32,
    pub bios: Option<String>,
    pub kernel: Option<String>,
    pub initrd: Option<String>,
    pub command_line: Option<String>,
    pub console: Console,
    pub uart_output: bool,
    pub drives: Vec<DriveConfig>,
    pub filesystems: Vec<FilesystemConfig>,
    pub networks: Vec<NetworkConfig>,
    pub display: Option<DisplayConfig>,
    pub input_device: Option<String>,
    pub rtc_local_time: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for ConfigError {}

impl From<ParseError> for ConfigError {
    fn from(value: ParseError) -> Self {
        Self(value.to_string())
    }
}

impl VmConfig {
    /// Parses and validates a version-one virtual machine configuration.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed input, missing required properties, or
    /// properties whose types or combinations violate the configuration schema.
    pub fn parse(source: &str) -> Result<Self, ConfigError> {
        let root = parse_value(source)?;
        let object = root
            .as_object()
            .ok_or_else(|| ConfigError("configuration must be an object".to_owned()))?;
        let version = required_integer(object, "version")?;
        if version != CONFIG_VERSION {
            return Err(ConfigError(format!(
                "unsupported configuration version {version}"
            )));
        }
        let machine = required_string(object, "machine")?.to_owned();
        let memory_size_mib = required_integer(object, "memory_size")?;
        let console = match optional_string(object, "console")? {
            None | Some("virtio") => Console::Virtio,
            Some("uart") => Console::Uart,
            Some(_) => return Err(ConfigError("console must be 'virtio' or 'uart'".to_owned())),
        };
        let mut drives = Vec::new();
        for index in 0..MAX_DRIVES {
            let name = format!("drive{index}");
            let Some(value) = object.get(&name) else {
                break;
            };
            let entry = require_object(value, &name)?;
            drives.push(DriveConfig {
                file: required_string(entry, "file")?.to_owned(),
                device: optional_string(entry, "device")?.map(str::to_owned),
            });
        }
        reject_over_limit(object, "drive", MAX_DRIVES)?;

        let mut filesystems = Vec::new();
        for index in 0..MAX_FILESYSTEMS {
            let name = format!("fs{index}");
            let Some(value) = object.get(&name) else {
                break;
            };
            let entry = require_object(value, &name)?;
            let file = optional_string(entry, "file")?;
            let socket = optional_string(entry, "socket")?;
            let js9p = optional_bool(entry, "js9p")?.unwrap_or(false);
            let backend = match (file, socket, js9p) {
                (Some(file), None, false) => FilesystemBackend::File(file.to_owned()),
                (None, Some(socket), false) => FilesystemBackend::Socket(socket.to_owned()),
                (None, None, true) => FilesystemBackend::JavaScript9p,
                _ => {
                    return Err(ConfigError(format!(
                        "{name} must select exactly one backend"
                    )));
                }
            };
            let tag =
                optional_string(entry, "tag")?.map_or_else(|| default_fs_tag(index), str::to_owned);
            filesystems.push(FilesystemConfig { backend, tag });
        }
        reject_over_limit(object, "fs", MAX_FILESYSTEMS)?;

        let mut networks = Vec::new();
        for index in 0..MAX_NETWORK_INTERFACES {
            let name = format!("eth{index}");
            let Some(value) = object.get(&name) else {
                break;
            };
            let entry = require_object(value, &name)?;
            let network_driver = required_string(entry, "driver")?.to_owned();
            let interface_name = if network_driver == "tap" {
                Some(required_string(entry, "ifname")?.to_owned())
            } else {
                optional_string(entry, "ifname")?.map(str::to_owned)
            };
            networks.push(NetworkConfig {
                driver: network_driver,
                interface_name,
            });
        }
        reject_over_limit(object, "eth", MAX_NETWORK_INTERFACES)?;

        let display = parse_display(object)?;
        Ok(Self {
            machine,
            memory_size_mib,
            bios: optional_string(object, "bios")?.map(str::to_owned),
            kernel: optional_string(object, "kernel")?.map(str::to_owned),
            initrd: optional_string(object, "initrd")?.map(str::to_owned),
            command_line: optional_string(object, "cmdline")?.map(str::to_owned),
            console,
            uart_output: optional_bool(object, "uart_output")?.unwrap_or(false),
            drives,
            filesystems,
            networks,
            display,
            input_device: optional_string(object, "input_device")?.map(str::to_owned),
            rtc_local_time: optional_bool(object, "rtc_local_time")?.unwrap_or(false),
        })
    }

    pub fn apply_command_line(&mut self, addition: &str) {
        if let Some(replacement) = addition.strip_prefix('!') {
            self.command_line = Some(replacement.to_owned());
            return;
        }
        let current = self.command_line.take().unwrap_or_default();
        self.command_line = Some(format!("{current} {addition}"));
    }
}

fn parse_display(object: &BTreeMap<String, Value>) -> Result<Option<DisplayConfig>, ConfigError> {
    object
        .get("display0")
        .map(|value| {
            let entry = require_object(value, "display0")?;
            Ok(DisplayConfig {
                device: required_string(entry, "device")?.to_owned(),
                width: required_integer(entry, "width")?,
                height: required_integer(entry, "height")?,
            })
        })
        .transpose()
}

#[must_use]
pub fn resolve_asset_path(config_path: Option<&str>, asset_path: &str) -> String {
    if config_path.is_none() || asset_path.contains(':') || asset_path.starts_with('/') {
        return asset_path.to_owned();
    }
    let config_path = config_path.unwrap_or_default();
    let Some(slash) = config_path.rfind('/') else {
        return asset_path.to_owned();
    };
    format!("{}{asset_path}", &config_path[..=slash])
}

fn default_fs_tag(index: usize) -> String {
    if index == 0 {
        "/dev/root".to_owned()
    } else {
        format!("/dev/root{index}")
    }
}

fn require_object<'a>(
    value: &'a Value,
    name: &str,
) -> Result<&'a BTreeMap<String, Value>, ConfigError> {
    value
        .as_object()
        .ok_or_else(|| ConfigError(format!("{name} must be an object")))
}

fn required_integer(object: &BTreeMap<String, Value>, name: &str) -> Result<i32, ConfigError> {
    object
        .get(name)
        .ok_or_else(|| ConfigError(format!("missing '{name}'")))?
        .as_integer()
        .ok_or_else(|| ConfigError(format!("{name} must be an integer")))
}

fn required_string<'a>(
    object: &'a BTreeMap<String, Value>,
    name: &str,
) -> Result<&'a str, ConfigError> {
    optional_string(object, name)?.ok_or_else(|| ConfigError(format!("missing '{name}'")))
}

fn optional_string<'a>(
    object: &'a BTreeMap<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, ConfigError> {
    object
        .get(name)
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| ConfigError(format!("{name} must be a string")))
        })
        .transpose()
}

fn optional_bool(
    object: &BTreeMap<String, Value>,
    name: &str,
) -> Result<Option<bool>, ConfigError> {
    object
        .get(name)
        .map(|value| match value {
            Value::Bool(value) => Ok(*value),
            _ => Err(ConfigError(format!("{name} must be a boolean"))),
        })
        .transpose()
}

fn reject_over_limit(
    object: &BTreeMap<String, Value>,
    prefix: &str,
    limit: usize,
) -> Result<(), ConfigError> {
    if object.contains_key(&format!("{prefix}{limit}")) {
        Err(ConfigError(format!("too many {prefix} entries")))
    } else {
        Ok(())
    }
}

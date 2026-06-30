# declare

[![github](https://img.shields.io/badge/github-mcmah309/declare-8da0cb?style=for-the-badge\&labelColor=555555\&logo=github)](https://github.com/mcmah309/declare)
[![crates.io](https://img.shields.io/crates/v/declare.svg?style=for-the-badge\&color=fc8d62\&logo=rust)](https://crates.io/crates/declare)
[![docs.rs](https://img.shields.io/badge/docs.rs-declare-66c2a5?style=for-the-badge\&labelColor=555555\&logo=docs.rs)](https://docs.rs/declare)
[![test status](https://img.shields.io/github/actions/workflow/status/mcmah309/declare/ci.yml?branch=master\&style=for-the-badge)](https://github.com/mcmah309/declare/actions/workflows/ci.yml)

`declare` provides macros for reducing boilerplate around common Rust declarations. Rather than introducing new syntax, it generates the code you would typically write by hand while keeping the source concise and explicit.

Currently it provides:

* **`newtype_variants`** — Extract enum variants into standalone structs while generating `From`/`TryFrom` implementations.
* **`common_accessors`** — Generate accessors for fields shared across enum variants.

## Example

```rust
#[declare::newtype_variants]
#[declare::common_accessors]
// #[declare::augment(newtype_variants, common_accessors)] // one liner
enum Message<'a> {
    #[newtype]
    #[derive(Debug)]
    Text {
        id: usize,
        body: &'a str,
    },
    #[newtype]
    Binary {
        id: usize,
        bytes: Vec<u8>,
    },
    Ping {
        id: usize,
    },
}
```

<details>

<summary>Expansion</summary>

```rust
enum Message<'a> {
    Text(Text<'a>),
    Binary(Binary),
    Ping { id: usize },
}
#[derive(Debug)]
struct Text<'a> {
    id: usize,
    body: &'a str,
}
struct Binary {
    id: usize,
    bytes: Vec<u8>,
}
impl<'a> ::core::convert::From<Text<'a>> for Message<'a> {
    fn from(value: Text<'a>) -> Self {
        Message::Text(value)
    }
}
impl<'a> ::core::convert::TryFrom<Message<'a>> for Text<'a> {
    type Error = Message<'a>;
    fn try_from(value: Message<'a>) -> ::core::result::Result<Self, Self::Error> {
        match value {
            Message::Text(inner) => Ok(inner),
            other => Err(other),
        }
    }
}
impl<'declare_internal, 'a> ::core::convert::TryFrom<&'declare_internal Message<'a>>
    for &'declare_internal Text<'a>
{
    type Error = &'declare_internal Message<'a>;
    fn try_from(
        value: &'declare_internal Message<'a>,
    ) -> ::core::result::Result<Self, Self::Error> {
        match value {
            Message::Text(inner) => Ok(inner),
            other => Err(other),
        }
    }
}
impl<'declare_internal, 'a> ::core::convert::TryFrom<&'declare_internal mut Message<'a>>
    for &'declare_internal mut Text<'a>
{
    type Error = &'declare_internal Message<'a>;
    fn try_from(
        value: &'declare_internal mut Message<'a>,
    ) -> ::core::result::Result<Self, Self::Error> {
        match value {
            Message::Text(inner) => Ok(inner),
            other => Err(other),
        }
    }
}
impl<'a> ::core::convert::From<Binary> for Message<'a> {
    fn from(value: Binary) -> Self {
        Message::Binary(value)
    }
}
impl<'a> ::core::convert::TryFrom<Message<'a>> for Binary {
    type Error = Message<'a>;
    fn try_from(value: Message<'a>) -> ::core::result::Result<Self, Self::Error> {
        match value {
            Message::Binary(inner) => Ok(inner),
            other => Err(other),
        }
    }
}
impl<'declare_internal, 'a> ::core::convert::TryFrom<&'declare_internal Message<'a>>
    for &'declare_internal Binary
{
    type Error = &'declare_internal Message<'a>;
    fn try_from(
        value: &'declare_internal Message<'a>,
    ) -> ::core::result::Result<Self, Self::Error> {
        match value {
            Message::Binary(inner) => Ok(inner),
            other => Err(other),
        }
    }
}
impl<'declare_internal, 'a> ::core::convert::TryFrom<&'declare_internal mut Message<'a>>
    for &'declare_internal mut Binary
{
    type Error = &'declare_internal Message<'a>;
    fn try_from(
        value: &'declare_internal mut Message<'a>,
    ) -> ::core::result::Result<Self, Self::Error> {
        match value {
            Message::Binary(inner) => Ok(inner),
            other => Err(other),
        }
    }
}
impl<'a> Message<'a> {
    fn id_ref(&self) -> &usize {
        match self {
            Message::Text(text) => &text.id,
            Message::Binary(binary) => &binary.id,
            Message::Ping { id, .. } => id,
        }
    }
    fn id_mut(&mut self) -> &mut usize {
        match self {
            Message::Text(text) => &mut text.id,
            Message::Binary(binary) => &mut binary.id,
            Message::Ping { id, .. } => id,
        }
    }
    fn into_id(self) -> usize {
        match self {
            Message::Text(text) => text.id,
            Message::Binary(binary) => binary.id,
            Message::Ping { id, .. } => id,
        }
    }
    fn body_ref(&self) -> Option<&str> {
        match self {
            Message::Text(text) => Some(text.body),
            Message::Binary(_) | Message::Ping { .. } => None,
        }
    }
    fn bytes_ref(&self) -> Option<&Vec<u8>> {
        match self {
            Message::Text(_) | Message::Ping { .. } => None,
            Message::Binary(binary) => Some(&binary.bytes),
        }
    }
    fn bytes_mut(&mut self) -> Option<&mut Vec<u8>> {
        match self {
            Message::Text(_) | Message::Ping { .. } => None,
            Message::Binary(binary) => Some(&mut binary.bytes),
        }
    }
    fn into_bytes(self) -> Option<Vec<u8>> {
        match self {
            Message::Text(_) | Message::Ping { .. } => None,
            Message::Binary(binary) => Some(binary.bytes),
        }
    }
}
```

</details>

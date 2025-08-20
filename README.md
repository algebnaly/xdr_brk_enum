# xdr_brk_enum
This is a proc-macro crate that provides a derive macro for serde::Serialize and serde::Deserialize on Rust enum types, to support XDR (External Data Representation) serialization and deserialization.

## Usage

Add this crate to your `Cargo.toml`:

```toml
[dependencies]
serde = { version = "1.0" }
xdr_brk_enum = "0.1"
xdr_brk = "0.1"
```

and in your Rust code:
```rust
use xdr_brk_enum::XDREnumSerialize;

const fn foo() -> u32 {
    42
}

#[repr(u32)]
#[derive(XDREnumSerialize)]
enum MyEnum {
    Variant1 = foo(),
    Variant2(String) = 0,
    Variant3 { field1: i32, field2: f64 } = 100,
}


fn main(){
    let my_enum = MyEnum::Variant1;
    let serialized = xdr_brk::to_bytes(&my_enum).unwrap();
    println!("Serialized: {:?}", serialized);
}

```

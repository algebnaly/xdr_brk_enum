use xdr_brk_enum::{XDREnumDeserialize, XDREnumSerialize};

const fn foo() -> u32 {
    42
}

#[repr(u32)]
#[derive(XDREnumSerialize, XDREnumDeserialize)]
enum MyEnum {
    Variant1 = foo(),
    Variant2(u32),
    Variant3 {
        a: u32,
        b: String,
    },
    #[default_arm]
    Variant4(u8),
}

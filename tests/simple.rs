
#[macro_use]
extern crate delta_encode;

use delta_encode::bitio::{self, DeltaEncodable};

#[test]
fn test_enum() {
    #[derive(Debug, DeltaEncode, PartialEq, Clone)]
    enum Testing {
        A,
        B,
        C(i32, u64, #[delta_bits="3"] u8),
        D {
            #[delta_bits = "16"]
            a: i32,
            #[delta_bits = "2"]
            b: u8,
            sub: SubTest,
        }
    }

    #[derive(Debug, DeltaEncode, PartialEq, Clone)]
    #[delta_complete]
    #[delta_always]
    struct SubTest {
        val: u8,
        val2: u16,
    }

    let test_val = Testing::D {
        a: 6,
        b: 1,
        sub: SubTest { val: 1, val2: 2 },
    };

    let mut output = bitio::Writer::new(vec![]);
    test_val.encode(None, &mut output).unwrap();
    let data = output.finish().unwrap();

    println!("{:?} => {:?}", test_val, data);

    let mut r = bitio::Reader::new(std::io::Cursor::new(data));
    let decoded_val = Testing::decode(None, &mut r).unwrap();
    assert_eq!(decoded_val, test_val);

    let changed = Testing::D {
        a: 3,
        b: 1,
        sub: SubTest { val: 1, val2: 2 },
    };

    let mut output = bitio::Writer::new(vec![]);
    changed.encode(Some(&test_val), &mut output).unwrap();
    let data = output.finish().unwrap();

    println!("{:?} => {:?}", changed, data);

    let mut r = bitio::Reader::new(std::io::Cursor::new(data));
    let decoded_val2 = Testing::decode(Some(&decoded_val), &mut r).unwrap();
    assert_eq!(decoded_val2, changed);
}

#[test]
fn simple() {
    #[derive(Debug, DeltaEncode, PartialEq, Clone)]
    struct Testing {
        #[delta_bits = "4"]
        a: i32,
        #[delta_bits = "16"]
        b: u32,
        #[delta_always]
        c: i8,
        sub: SubTest,
        tuple: Tuple,

        #[delta_subbits = "5, 8, 10, 16"]
        subbits: u32,

        #[delta_diff]
        #[delta_subbits = "5, 8, 10, 16"]
        diff_subbits: i32,

        array: [i32; 5],
    }

    #[derive(Debug, DeltaEncode, PartialEq, Clone)]
    #[delta_complete]
    #[delta_always]
    struct SubTest {
        val: u8,
        val2: u16,
    }

    #[derive(Debug, DeltaEncode, PartialEq, Clone)]
    struct Tuple(u8, u8);

    let test_val = Testing {
        a: 4,
        b: 88,
        c: -4,
        subbits: 5,
        diff_subbits: -20,
        tuple: Tuple(1, 2),
        sub: SubTest {
            val: 64,
            val2: 31,
        },
        array: [1, 2, 3, 4, 5],
    };

    let mut output = bitio::Writer::new(vec![]);
    test_val.encode(None, &mut output).unwrap();
    let data = output.finish().unwrap();

    println!("{:?} => {:?}", test_val, data);

    let mut r = bitio::Reader::new(std::io::Cursor::new(data));
    let decoded_val = Testing::decode(None, &mut r).unwrap();
    assert_eq!(decoded_val, test_val);

    let changed = Testing {
        b: 31,
        c: 54,
        subbits: 0xFFF,
        diff_subbits: 40,
        array: [2, 3, 3, 4, 5],
        .. test_val.clone()
    };

    let mut output = bitio::Writer::new(vec![]);
    changed.encode(Some(&test_val), &mut output).unwrap();
    let data = output.finish().unwrap();

    println!("{:?} => {:?}", changed, data);

    let mut r = bitio::Reader::new(std::io::Cursor::new(data));
    let decoded_val2 = Testing::decode(Some(&decoded_val), &mut r).unwrap();
    assert_eq!(decoded_val2, changed);
}

#[test]
fn floats() {
    #[derive(Debug, DeltaEncode, PartialEq, Clone)]
    struct TestFloats {
        full: f32,

        #[delta_fixed]
        #[delta_bits = "6:4"]
        fixed: f32,

        #[delta_fixed]
        #[delta_subbits = "6:4,10:4,-1:-1"]
        fixed_sub: f32,

        #[delta_fixed]
        #[delta_diff]
        #[delta_subbits = "4:5,6:5,10:5,16:5,-1:-1"]
        fixed_sub_diff: f32,
    }

    let test_val = TestFloats {
        full: 5.8,

        fixed: 3.2,
        fixed_sub: 5.6,
        fixed_sub_diff: 18.5,
    };

    let mut output = bitio::Writer::new(vec![]);
    test_val.encode(None, &mut output).unwrap();
    let data = output.finish().unwrap();

    println!("{:?} => {:?}", test_val, data);

    let mut r = bitio::Reader::new(std::io::Cursor::new(data));
    let decoded_val = TestFloats::decode(None, &mut r).unwrap();

    println!("{:?}", decoded_val);

    let changed = TestFloats {
        full: 5.8,
        fixed: 20.5,
        fixed_sub: 50.6,
        fixed_sub_diff: 18.5,
        .. test_val.clone()
    };

    let mut output = bitio::Writer::new(vec![]);
    changed.encode(Some(&test_val), &mut output).unwrap();
    let data = output.finish().unwrap();

    println!("{:?} => {:?}", changed, data);

    let mut r = bitio::Reader::new(std::io::Cursor::new(data));
    let decoded_val2 = TestFloats::decode(Some(&decoded_val), &mut r).unwrap();
    println!("{:?}", decoded_val2);
}
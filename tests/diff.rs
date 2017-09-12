extern crate ddbug;

fn diff(name: &str, expect: &str) {
    let mut options = options();
    options.unit("src/diff.c").name(name);
    let mut diff = Vec::new();
    ddbug::File::parse("tests/bin/diff1", &mut |output_1| {
        ddbug::File::parse("tests/bin/diff2", &mut |output_2| {
            ddbug::File::diff(&mut diff, output_1, output_2, &options)
        })
    }).unwrap();
    let diff = String::from_utf8(diff).unwrap();
    if !equal(&diff, expect) {
        println!("\nDiff:");
        println!("{}", diff);
        println!("Expected:");
        println!("{}", expect);
        assert_eq!(diff, expect);
    }
}

fn options<'a>() -> ddbug::Options<'a> {
    ddbug::Options {
        inline_depth: 1,

        category_unit: false,
        category_type: true,
        category_function: true,
        category_variable: true,

        filter_name: None,
        filter_namespace: Vec::new(),
        filter_unit: None,

        sort: ddbug::Sort::None,

        ignore_function_address: true,
        ignore_variable_address: true,
        ..Default::default()
    }
}

fn equal(mut diff: &str, expect: &str) -> bool {
    let mut expects = expect.split("[..]");
    if let Some(e) = expects.next() {
        if !diff.starts_with(e) {
            return false;
        }
        diff = &diff[e.len()..];
    }
    for e in expects {
        loop {
            if diff.starts_with(e) {
                diff = &diff[e.len()..];
                break;
            }
            if diff.is_empty() {
                return false;
            }
            diff = &diff[1..];
        }
    }
    diff.is_empty()
}

macro_rules! test {
    ($name:ident, $($val:expr),*) => {
        #[test]
        fn $name() {
            let expect = concat!($($val),*);
            diff(stringify!($name), expect);
        }
    }
}

include!("src/diff.rs");

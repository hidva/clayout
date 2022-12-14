use anyhow::ensure;
use clap::Parser;
use log::{info, warn};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

const BITS_PER_BYTE: u64 = 8;

fn bit2byte(input: u64) -> u64 {
    (input + BITS_PER_BYTE - 1) / BITS_PER_BYTE
}
static ID_GENERATOR: AtomicU64 = AtomicU64::new(0);
fn uniq_id() -> u64 {
    ID_GENERATOR.fetch_add(1, Relaxed)
}

fn is_ident_char(ch: char) -> bool {
    return ch == '_' || ch.is_alphanumeric();
}
fn ident_part(name: &str) -> &str {
    let Some(bad_idx) = name.find(|c|!is_ident_char(c)) else {
        return name;
    };
    return &name[0..bad_idx];
}

fn consume_ident_chars(out: &mut String, input: &mut impl Iterator<Item = char>) -> Option<char> {
    while let Some(ch) = input.next() {
        if is_ident_char(ch) {
            out.push(ch);
        } else {
            return Some(ch);
        }
    }
    return None;
}

// '::std::_Rb_tree_key_compare<std::less<niagara::TabletEventListener *> >' -> ['std', '_Rb_tree_key_compare<std::less<niagara::TabletEventListener *> >'].
fn parse_typename(input: &str) -> anyhow::Result<Vec<String>> {
    let mut ret = Vec::<String>::new();
    let mut add_part = |p: String| {
        if !p.is_empty() {
            ret.push(p);
        }
    };
    let mut iter = input.chars();
    loop {
        let mut part = String::new();
        let next_ch = consume_ident_chars(&mut part, &mut iter);
        let Some(next_ch) = next_ch else {
            add_part(part);
            return Ok(ret);
        };
        if next_ch == ':' {
            add_part(part);
            let next_ch = iter.next();
            ensure!(next_ch == Some(':'), "invalid input symbol");
            continue;
        }
        part.push(next_ch);
        while let Some(ch) = iter.next() {
            part.push(ch);
        }
        add_part(part);
        return Ok(ret);
    }
}

#[derive(Parser)]
#[clap(author, version, about)]
struct Args {
    /// input so path, can specify more than once
    #[arg(short = 'i')]
    so_path: Vec<String>,

    /// input file path, each line represents a so path, can specify more than once
    #[arg(short = 'I')]
    so_file_path: Vec<String>,

    /// output file path
    #[arg(short)]
    out_path: String,

    /// type name, such as 'namespace1::namespace2::TypeName'
    #[arg(value_parser=parse_typename)]
    dest: Vec<Vec<String>>,
}

impl Args {
    fn is_dest(&self, tyn: &parser::TypeName) -> bool {
        for d in &self.dest {
            if tyn.ends_with(&d) {
                return true;
            }
        }
        return false;
    }
}

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<std::fs::File>>>
where
    P: AsRef<std::path::Path>,
{
    let file = std::fs::File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

// is_declaration ????????? ty ????????????????????????????????????,
fn is_declaration(ty: &parser::Type) -> bool {
    match ty.kind() {
        parser::TypeKind::Struct(s) => s.is_declaration(),
        parser::TypeKind::Union(s) => s.is_declaration(),
        parser::TypeKind::Enumeration(s) => s.is_declaration(),
        parser::TypeKind::Unspecified(_) => true,
        _ => false,
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq, Debug)]
struct TypeIndex {
    // input_id ??? inputs_hash ????????????,
    input_id: usize,
    typoff: parser::TypeOffset,
}

// ????????????????????? padding. ??????????????? struct/union ?????? packed __attribute__,
// ??????????????????:
// struct S1218 {
//   long l;
//   ch c;
// };
//
// struct A1218: public S1218 {
//   // ??????, ?????? A.i ?????????????????? S padding ????????????.
//   // S off=0, size=16
//   // i off=12, size=4
//   int i;
// }
#[derive(Debug)]
struct TypeInfo {
    // name ???????????? C ???????????????????????????.
    // ????????????????????? `[struct|union|enum] ?????????[*]*`.
    name: String,
    // packed size ????????? attribute packed ????????? size,
    // size ??? dwarf ???????????? type size.
    // ??? S1218 ??????, packed_size = 9, size = 16.
    packed_size: u64,
    size: u64,
}

impl TypeInfo {
    fn ident(&self) -> &str {
        ident_part(self.name.split_whitespace().last().unwrap())
    }
}

type ProcessState = HashMap<TypeIndex, Option<Rc<TypeInfo>>>;

struct Printer {
    h_file: std::fs::File,
    c_file: std::fs::File,
    used_idents: HashMap<String, u64>,
}

impl Printer {
    fn do_add_eq_assert(&mut self, expr: &str, size: u64) -> io::Result<()> {
        writeln!(self.c_file, "  ZHANYI_HIDVA_ASSERT_EQ({}, {});", expr, size)
    }
}
impl Printer {
    fn try_open(path: &str) -> io::Result<Printer> {
        const ASSERT_EQ_DEF: &'static str = r###"
#define ZHANYI_HIDVA_ASSERT_EQ(a, e) do {    \
    int actual_size = (a);  \
    int expect_size = (e);  \
    if (actual_size != expect_size) {   \
        fprintf(stderr, "ASSERT FAILED! actual: %s, which is %d; expect: %s, which is %d\n", #a, actual_size, #e, expect_size);    \
        abort();    \
    }   \
} while(0)
        "###;
        let mut h_file_name = path.to_string();
        h_file_name.push_str(".h");
        let mut c_file_name = path.to_string();
        c_file_name.push_str(".c");
        let mut h_file = std::fs::File::create(&h_file_name)?;
        let mut c_file = std::fs::File::create(c_file_name)?;
        writeln!(h_file, "// Generated by hidva/clayout! ????????????!")?;
        writeln!(h_file, "#pragma once")?;
        writeln!(h_file, "#include <linux/types.h>")?;
        writeln!(c_file, "// Generated by hidva/clayout! ????????????!")?;
        writeln!(c_file, "#include <stdio.h>")?;
        writeln!(c_file, "#include <stdlib.h>")?;
        writeln!(c_file, "#include \"{}\"", &h_file_name)?;
        writeln!(c_file, "\n\n\n")?;
        writeln!(c_file, "{}", ASSERT_EQ_DEF)?;
        writeln!(c_file, "\n\n\n")?;
        writeln!(c_file, "int main() {{")?;
        Ok(Printer {
            h_file,
            c_file,
            used_idents: HashMap::new(),
        })
    }

    fn add_eq_assert(&mut self, expr: &str, size: u64) -> io::Result<()> {
        self.do_add_eq_assert(expr, size)?;
        writeln!(self.c_file, "")
    }

    fn add_eq_asserts(&mut self, asserts: &[EqAssert]) -> io::Result<()> {
        for eq_assert in asserts {
            self.do_add_eq_assert(&eq_assert.expr, eq_assert.val)?;
        }
        writeln!(self.c_file, "")
    }

    // ????????? .h ????????????????????????, ???????????? alloc_ident ?????????. ?????? add_type ????????????.
    fn alloc_ident(&mut self, tyname: &parser::TypeName) -> String {
        // return val may be empty
        fn get_ident_part(name: Option<&str>) -> &str {
            name.map(|v| ident_part(v)).unwrap_or("")
        }
        let mut idents = 'get_idents: {
            let mut idents = Vec::new();
            let ident_part = get_ident_part(tyname.name);
            if ident_part.is_empty() {
                break 'get_idents idents;
            }
            idents.push(ident_part);

            let mut ns_opt = tyname.namespace;
            while let Some(ns) = ns_opt {
                let ident = get_ident_part(ns.name());
                if !ident.is_empty() {
                    idents.push(ident);
                }
                ns_opt = ns.parent();
            }
            break 'get_idents idents;
        };
        if idents.is_empty() {
            return format!("AnonType{}", uniq_id());
        }
        idents.reverse();

        let mut test_idx = idents.len() - 1;
        loop {
            let test_ident = idents[test_idx..].join("_");
            let Some(used) = self.used_idents.get_mut(&test_ident) else {
                self.used_idents.insert(test_ident.clone(), 0);
                return test_ident;
            };
            if test_idx == 0 {
                *used += 1;
                return format!("{}_{}", test_ident, *used);
            }
            test_idx -= 1;
        }
    }

    fn add_type(&mut self, lines: &[String]) -> io::Result<()> {
        for l in lines {
            writeln!(self.h_file, "{}", l)?;
        }
        writeln!(self.h_file, "")?;
        writeln!(self.h_file, "")?;
        return Ok(());
    }

    fn finish(&mut self) -> io::Result<()> {
        writeln!(self.c_file, "  return 0;")?;
        writeln!(self.c_file, "}}")?;
        Ok(())
    }
}

struct Member {
    off: u64,
    len: u64,
    // def, ?????? `__u8 __padding33[3]`; field_name ?????? __padding33.
    field_name: String,
    // ????????????????????????.
    def: String,
    is_padding: bool, // ?????? new_padding() ??????????????? true.
}

impl Member {
    // ????????? def ?????? 2 ???????????????.
    fn print(&self, tyname: &str, def: &mut Vec<String>, asserts: &mut Vec<EqAssert>) {
        def.push(format!("  {};", &self.def));

        asserts.push(EqAssert {
            expr: format!("(long int)(&((({}*)0)->{}))", tyname, self.field_name),
            val: self.off,
        });
        asserts.push(EqAssert {
            expr: format!("sizeof((({}*)0)->{})", tyname, self.field_name),
            val: self.len,
        });
    }

    fn new_padding(off: u64, len: u64) -> Self {
        let field_name = format!("__padding{}", uniq_id());
        Self {
            off,
            len,
            def: format!("__u8 {}[{}]", &field_name, len),
            field_name,
            is_padding: true,
        }
    }

    fn new_placeholder(off: u64, len: u64, name: &str) -> Self {
        let field_name = format!("{}{}", name, uniq_id());
        Self {
            off,
            len,
            def: format!("__u8 {}[{}]", &field_name, len),
            field_name,
            is_padding: false,
        }
    }
}

// ???????????????????????????????????? process state ????????????.
fn get_type_info(
    processed: &mut ProcessState,
    printer: &mut Printer,
    tyidx: TypeIndex,
    ty_max_size: Option<u64>,
    inputs_hash: &[parser::FileHash],
    type_db: &HashMap<parser::TypeName, TypeIndex>,
) -> io::Result<Option<Rc<TypeInfo>>> {
    let tyinfo = match processed.get(&tyidx).map(|v| v.clone()) {
        Some(i) => i,
        None => {
            process_type(processed, printer, tyidx, ty_max_size, inputs_hash, type_db)?;
            processed.get(&tyidx).unwrap().clone()
        }
    };
    debug_assert!(tyinfo
        .as_ref()
        .map(|v| v.packed_size <= v.size)
        .unwrap_or(true));
    if let (Some(tyinfo), Some(max_size)) = (&tyinfo, ty_max_size) {
        assert!(tyinfo.packed_size <= max_size);
    }
    return Ok(tyinfo);
}

// ?????? tyidx ??? real_tyidx ???????????????, tyidx ---> real_tyidx.
fn handle_sym_link(
    processed: &mut ProcessState,
    printer: &mut Printer,
    tyidx: TypeIndex,
    real_tyidx: TypeIndex,
    ty_max_size: Option<u64>,
    inputs_hash: &[parser::FileHash],
    type_db: &HashMap<parser::TypeName, TypeIndex>,
) -> io::Result<()> {
    let tyinfo = get_type_info(
        processed,
        printer,
        real_tyidx,
        ty_max_size,
        inputs_hash,
        type_db,
    )?;
    processed.insert(tyidx, tyinfo);
    return Ok(());
}

// ?????? >= start ??????, ????????? bit_offset ??? BYTE ????????????????????????, ?????????????????? None.
fn find_next_idx(tylayout: &Vec<parser::Layout>, start: usize) -> Option<usize> {
    for idx in start..tylayout.len() {
        if (tylayout[idx].bit_offset % BITS_PER_BYTE) == 0 {
            return Some(idx);
        }
    }
    return None;
}

// check ok return bit size.
// #1 ???????????? C++ ??????:
//   struct S {};
//   struct F: public S { int i ; };  // ?????? F.i ??? S ???????????????????????????.
// #2 ????????? C++ ??????:
//   struct S {
//     long l;
//     char ch[0];  // ?????? ch bit_size None.
//   };
// fn check_layout(tylayout: &Vec<parser::Layout>) -> Option<u64> {
//     let iter = tylayout.iter();
//     let Some(mut prev) = iter.next() else {
//         return None;
//     };
//     while let Some(curr) = iter.next() {
//         let Some(prevsize) = prev.bit_size.get() else {
//             return None;
//         };
//         if curr.bit_offset == prev.bit_offset ||  // #1
//            curr.bit_offset == prev.bit_offset + prevsize
//         {
//             prev = curr;
//             continue;
//         }
//         return None;
//     }
//     let prevsize = match prev.bit_size.get() {
//         Some(v) => v,
//         None => 0, // #2
//     };
//     return Some(prev.bit_offset + prevsize);
// }

// ?????????????????? `long l:32` ????????????...
fn is_bitfield(l: &parser::Layout) -> bool {
    let Some(s) = l.bit_size.get() else {
        return false;
    };
    return s % BITS_PER_BYTE != 0;
}

fn is_valid_ident(input: &str) -> bool {
    let ret = input.trim_end_matches(is_ident_char);
    return ret.is_empty();
}

fn member_name(input: Option<&str>) -> Cow<str> {
    let Some(input) = input else {
        return Cow::Owned(format!("__anon{}", uniq_id()));
    };
    if is_valid_ident(input) {
        return Cow::Borrowed(input);
    }
    return Cow::Owned(format!("__mem{}", uniq_id()));
}

struct EqAssert {
    expr: String,
    val: u64,
}

// tydef, ?????? `union U`, `struct S` ??????,
// tymems ???????????? tymem off + len ??? ty_size.
fn process_members(
    processed: &mut ProcessState,
    printer: &mut Printer,
    tyidx: TypeIndex,
    tyname: &parser::TypeName,
    tymems: &[Member],
    tydef: &str,
    tysize: Option<u64>,
) -> io::Result<()> {
    let mut asserts = Vec::<EqAssert>::new();
    let mut struct_def = Vec::<String>::new();

    struct_def.push(format!("// tyname={} tyidx={:?}", tyname, tyidx));
    struct_def.push(format!("{} {{", tydef));
    for tymem in tymems {
        tymem.print(tydef, &mut struct_def, &mut asserts);
    }
    struct_def.push("} __attribute__((__packed__));".to_string());

    let Some(packed_size) = tymems.last().map(|v|v.off + v.len) else {
        return Ok(());
    };
    asserts.push(EqAssert {
        expr: format!("sizeof({})", tydef),
        val: packed_size,
    });

    printer.add_type(&struct_def)?;
    printer.add_eq_asserts(&asserts)?;
    if let Some(tysize) = tysize {
        processed.insert(
            tyidx,
            Some(Rc::new(TypeInfo {
                name: tydef.to_string(),
                packed_size,
                size: tysize,
            })),
        );
    }
    return Ok(());
}

fn process_union_type(
    processed: &mut ProcessState,
    printer: &mut Printer,
    tyidx: TypeIndex,
    ty: &parser::UnionType,
    // ty_max_size ??????????????? C++ ??? A1218, S1218 ????????????????????? padding ??????,
    // union ??????????????????, ?????????????????? ty_max_size.
    ty_max_size: Option<u64>,
    inputs_hash: &[parser::FileHash],
    type_db: &HashMap<parser::TypeName, TypeIndex>,
) -> io::Result<()> {
    let tyname = ty.type_name();
    if ty.is_declaration() {
        let Some(&real_tyidx) = type_db.get(&tyname) else {
            warn!("process_union_type: unknown declaration union. typidx={:?} typname={}", tyidx, &tyname);
            return Ok(());
        };
        return handle_sym_link(
            processed,
            printer,
            tyidx,
            real_tyidx,
            ty_max_size,
            inputs_hash,
            type_db,
        );
    }
    let Some(ty_size) = ty.byte_size() else {
        warn!("process_union_type: unknown byte size typidx={:?} typname={}", tyidx, ty.type_name());
        return Ok(());
    };
    if let Some(ty_max_size) = ty_max_size {
        if ty_size > ty_max_size {
            warn!(
                "process_union_type: invalid byte size typidx={:?} typname={} expect={} actual={}",
                tyidx,
                ty.type_name(),
                ty_max_size,
                ty_size
            );
            return Ok(());
        }
    }

    let mut tymems = Vec::<Member>::new();
    for union_mem in ty.members() {
        if union_mem.bit_offset() != 0 {
            warn!(
                "process_union_type: union_mem.bit_offset != 0! typidx={:?} typname={} member={:?}",
                tyidx,
                ty.type_name(),
                union_mem
            );
            return Ok(());
        }
        let Some(union_mem_bit_size) = union_mem.bit_size(&inputs_hash[tyidx.input_id]) else {
            warn!("process_union_type: unknown member size! typidx={:?} typname={} member={:?}", tyidx, ty.type_name(), union_mem);
            return Ok(());
        };
        let member_size = bit2byte(union_mem_bit_size);
        let tylayout = &parser::Layout {
            bit_offset: 0,
            bit_size: parser::Size::new(union_mem_bit_size),
            item: parser::LayoutItem::Member(union_mem),
        };

        if is_bitfield(tylayout) {
            tymems.push(Member::new_placeholder(0, member_size, "__bitfield"));
            continue;
        }

        let member_tyoff = union_mem.type_offset();
        let member_name = member_name(union_mem.name());
        let mem_tyidx = TypeIndex {
            input_id: tyidx.input_id,
            typoff: member_tyoff,
        };
        let mem_tyinfo = get_type_info(
            processed,
            printer,
            mem_tyidx,
            Some(member_size),
            inputs_hash,
            type_db,
        )?;
        let Some(mem_tyinfo) = mem_tyinfo else {
            tymems.push(Member::new_placeholder(0, member_size, "__unknown_type"));
            continue;
        };
        debug_assert!(mem_tyinfo.packed_size <= member_size);

        tymems.push(Member {
            off: 0,
            len: mem_tyinfo.packed_size,
            def: format!("{} {}", &mem_tyinfo.name, &member_name),
            field_name: member_name.into_owned(),
            is_padding: false,
        });
    }
    tymems.push(Member::new_placeholder(0, ty_size, "__HIDVA_dont_use"));

    let tydef = format!("union {}", printer.alloc_ident(&tyname));
    return process_members(
        processed,
        printer,
        tyidx,
        &ty.type_name(),
        &tymems,
        &tydef,
        Some(ty_size),
    );
}

fn process_struct_type(
    processed: &mut ProcessState,
    printer: &mut Printer,
    tyidx: TypeIndex,
    ty: &parser::StructType,
    ty_max_size: Option<u64>,
    inputs_hash: &[parser::FileHash],
    type_db: &HashMap<parser::TypeName, TypeIndex>,
) -> io::Result<()> {
    let tyname = ty.type_name();
    if ty.is_declaration() {
        let Some(&real_tyidx) = type_db.get(&tyname) else {
            warn!("process_struct_type: unknown declaration struct. typidx={:?} typname={}", tyidx, tyname);
            return Ok(());
        };
        return handle_sym_link(
            processed,
            printer,
            tyidx,
            real_tyidx,
            ty_max_size,
            inputs_hash,
            type_db,
        );
    }

    let Some(mut ty_bit_size) = ty.bit_size() else {
        warn!("process_struct_type: unknown type size: tyidx={:?} tyname={}", tyidx, ty.type_name());
        return Ok(());
    };
    let ty_dwarf_size = ty.byte_size().unwrap();
    let mut tylayout = ty.layout(&inputs_hash[tyidx.input_id]);
    while let Some(lastlayout) = tylayout.last() {
        if let parser::LayoutItem::Padding = lastlayout.item {
            let s = lastlayout.bit_size.get().unwrap();
            debug_assert!(ty_bit_size >= s);
            ty_bit_size -= s;
            tylayout.pop();
        } else {
            break;
        }
    }
    let tylayout = tylayout;
    let tysize = bit2byte(ty_bit_size);
    let ty_max_size = match ty_max_size {
        Some(v) => {
            if v > tysize {
                tysize
            } else {
                v
            }
        }
        None => tysize,
    };

    let mut tymems = Vec::<Member>::with_capacity(tylayout.len());
    let mut next_idx = find_next_idx(&tylayout, 0);
    debug_assert_eq!(next_idx.unwrap_or(0), 0);
    while let Some(item_idx) = next_idx {
        debug_assert_eq!(tylayout[item_idx].bit_offset % BITS_PER_BYTE, 0);
        debug_assert!(item_idx != 0 || tylayout[item_idx].bit_offset == 0); // layout() ?????????????????? offset: 0 ??????.
        next_idx = find_next_idx(&tylayout, item_idx + 1);

        let member_off = tylayout[item_idx].bit_offset / BITS_PER_BYTE;
        // member_size item_idx ???????????????, ???????????????.
        // ????????? member_size ??????, ????????? tylayout[item_idx].bit_size. ?????? S1218, A1218 ??????.
        let member_size = next_idx
            .map(|v| tylayout[v].bit_offset / BITS_PER_BYTE)
            .unwrap_or(ty_max_size)
            - member_off;
        // debug_assert!(tylayout[item_idx].bit_size.get().map(|v| v <= member_size).unwrap_or(true));
        if member_size <= 0 && next_idx.is_some() {
            continue;
        }
        // member_size == 0 && next_idx.is_none() ????????? member ?????????????????????, ??????????????????:
        //   struct S {int i; char ch[0];}
        // ?????? ch member_size = 0.

        if is_bitfield(&tylayout[item_idx]) {
            // ??????????????????, ?????? tymem ?????????????????? padding ??????,
            //   struct S { long i: 2; };
            //   struct A: public S {char ch;};
            // ?????????????????? A.ch ??????????????? S padding ???, ?????????????????????.
            tymems.push(Member::new_placeholder(
                member_off,
                member_size,
                "_bitfield",
            ));
            continue;
        }

        let (member_tyoff, member_name) = match tylayout[item_idx].item {
            parser::LayoutItem::Padding => {
                tymems.push(Member::new_padding(member_off, member_size));
                continue;
            }
            parser::LayoutItem::Member(mem) => (mem.type_offset(), member_name(mem.name())),
            parser::LayoutItem::Inherit(mem) => (
                mem.type_offset(),
                Cow::Owned(format!("__parent{}", uniq_id())),
            ),
            parser::LayoutItem::VariantPart(_) => {
                tymems.push(Member::new_placeholder(
                    member_off,
                    member_size,
                    "__variant_part",
                ));
                continue;
            }
        };
        let mem_tyidx = TypeIndex {
            input_id: tyidx.input_id,
            typoff: member_tyoff,
        };
        let mem_tyinfo = get_type_info(
            processed,
            printer,
            mem_tyidx,
            Some(member_size),
            inputs_hash,
            type_db,
        )?;
        let Some(mem_tyinfo) = mem_tyinfo else {
            tymems.push(Member::new_placeholder(member_off, member_size, "__unknown_type"));
            continue;
        };
        debug_assert!(mem_tyinfo.packed_size <= member_size);

        tymems.push(Member {
            off: member_off,
            len: mem_tyinfo.packed_size,
            def: format!("{} {}", &mem_tyinfo.name, &member_name),
            field_name: member_name.into_owned(),
            is_padding: false,
        });
        if mem_tyinfo.packed_size < member_size {
            tymems.push(Member::new_padding(
                member_off + mem_tyinfo.packed_size,
                member_size - mem_tyinfo.packed_size,
            ));
        }
    }
    while let Some(member) = tymems.last() {
        if member.is_padding {
            tymems.pop();
        } else {
            break;
        }
    }

    let tydef = format!("struct {}", printer.alloc_ident(&tyname));
    return process_members(
        processed,
        printer,
        tyidx,
        &ty.type_name(),
        &tymems,
        &tydef,
        Some(ty_dwarf_size),
    );
}

fn process_enum_type(
    processed: &mut ProcessState,
    printer: &mut Printer,
    tyidx: TypeIndex,
    ty: &parser::EnumerationType,
    ty_max_size: Option<u64>,
    inputs_hash: &[parser::FileHash],
    type_db: &HashMap<parser::TypeName, TypeIndex>,
) -> io::Result<()> {
    let tyname = ty.type_name();
    if ty.is_declaration() {
        let Some(&real_tyidx) = type_db.get(&tyname) else {
            warn!("process_enum_type: unknown declaration. typidx={:?} typname={}", tyidx, &tyname);
            return Ok(());
        };
        return handle_sym_link(
            processed,
            printer,
            tyidx,
            real_tyidx,
            ty_max_size,
            inputs_hash,
            type_db,
        );
    }
    let Some(ty_size) = ty.byte_size(&inputs_hash[tyidx.input_id]) else {
        warn!("process_enum_type: unknown byte size. typidx={:?} typname={}", tyidx, ty.type_name());
        return Ok(());
    };
    if let Some(ty_max_size) = ty_max_size {
        if ty_size > ty_max_size {
            warn!(
                "process_enum_type: invalid byte size. typidx={:?} typname={} expect={} actual={}",
                tyidx,
                ty.type_name(),
                ty_max_size,
                ty_size
            );
            return Ok(());
        }
    }

    // EnumerationType::ty may be none, ?????????????????????????????????.
    let ty_repr = if ty_size == 8 {
        "__s64"
    } else if ty_size == 4 {
        "__s32"
    } else if ty_size == 2 {
        "__s16"
    } else if ty_size == 1 {
        "__s8"
    } else {
        warn!(
            "process_enum_type: invalid byte size. typidx={:?} typname={} expect=8/4/2/1 actual={}",
            tyidx,
            ty.type_name(),
            ty_size
        );
        return Ok(());
    };

    let mut asserts = Vec::<EqAssert>::new();
    let mut struct_def = Vec::<String>::new();
    asserts.push(EqAssert {
        expr: format!("sizeof({})", ty_repr),
        val: ty_size,
    });
    let tydef = printer.alloc_ident(&tyname);
    struct_def.push(format!("// --- enum {} begin ---", &tydef));
    for enum_item in &ty.enumerators(&inputs_hash[tyidx.input_id]) {
        struct_def.push(format!(
            "// {}={}",
            enum_item.name().unwrap_or("<unknown enum item>"),
            enum_item.value().unwrap_or(-20181218),
        ));
    }
    struct_def.push(format!("// --- enum {} end ---", &tydef));
    struct_def.push(format!("typedef {} {};", ty_repr, &tydef));

    processed.insert(
        tyidx,
        Some(Rc::new(TypeInfo {
            name: tydef,
            packed_size: ty_size,
            size: ty_size,
        })),
    );
    printer.add_type(&struct_def)?;
    printer.add_eq_asserts(&asserts)?;
    return Ok(());
}

fn process_array_type(
    processed: &mut ProcessState,
    printer: &mut Printer,
    tyidx: TypeIndex,
    ty: &parser::ArrayType,
    ty_max_size: Option<u64>,
    inputs_hash: &[parser::FileHash],
    type_db: &HashMap<parser::TypeName, TypeIndex>,
) -> io::Result<()> {
    let mem_tyidx = TypeIndex {
        input_id: tyidx.input_id,
        typoff: ty.ty,
    };
    let mem_tyinfo = get_type_info(processed, printer, mem_tyidx, None, inputs_hash, type_db)?;
    let Some(mem_tyinfo) = mem_tyinfo else {
        warn!("process_array_type: unknown element type: tyidx={:?} ty={:?}", tyidx, ty);
        return Ok(());
    };

    let mem_tyident = mem_tyinfo.ident();
    let mut mem_tyname = mem_tyinfo.name.clone();
    if mem_tyinfo.size > mem_tyinfo.packed_size {
        // ??? S1218 ??????, ??????????????? S1218 ??? packed ???, ????????? S1218 ?????? array element, ???
        // ????????????????????? padding.
        mem_tyname = {
            let name = format!("{}_Padded", mem_tyident);
            let n = printer.alloc_ident(&parser::TypeName {
                namespace: None,
                name: Some(&name),
            });
            format!("struct {}", n)
        };
        let mut members = Vec::<Member>::new();
        let data_name = "data";
        members.push(Member {
            off: 0,
            len: mem_tyinfo.packed_size,
            field_name: data_name.to_string(),
            def: format!("{} {}", mem_tyinfo.name, data_name),
            is_padding: false,
        });
        members.push(Member::new_padding(
            mem_tyinfo.packed_size,
            mem_tyinfo.size - mem_tyinfo.packed_size,
        ));
        process_members(
            processed,
            printer,
            tyidx,
            &parser::TypeName {
                namespace: None,
                name: Some("padding struct"),
            },
            &members,
            &mem_tyname,
            None,
        )?;
    }

    let ele_cnt = if ty_max_size == Some(0) {
        0
    } else {
        let Some(array_byte_size) = ty.byte_size(&inputs_hash[tyidx.input_id]) else {
            warn!("process_array_type: unknown array size: tyidx={:?} ty={:?}", tyidx, ty);
            return Ok(());
        };
        if let Some(max_size) = ty_max_size {
            if max_size < array_byte_size {
                return Ok(());
            }
        }
        let Some(ele_count) = ty.count(&inputs_hash[tyidx.input_id]) else {
            warn!("process_array_type: unknown element count: tyidx={:?} ty={:?}", tyidx, ty);
            return Ok(());
        };
        if array_byte_size % ele_count != 0 || mem_tyinfo.size != array_byte_size / ele_count {
            warn!(
                "process_array_type: invalid array def: tyidx={:?} ty={:?}",
                tyidx, ty
            );
            return Ok(());
        }
        ele_count
    };
    let array_name = {
        let name = format!("{}_Array{}", mem_tyident, ele_cnt);
        let tyname = parser::TypeName {
            namespace: None,
            name: Some(&name),
        };
        printer.alloc_ident(&tyname)
    };
    let array_size = mem_tyinfo.size * ele_cnt;

    printer.add_type(&[format!(
        "typedef {} {}[{}];",
        mem_tyname, array_name, ele_cnt
    )])?;
    printer.add_eq_assert(&format!("sizeof({})", array_name), array_size)?;
    processed.insert(
        tyidx,
        Some(Rc::new(TypeInfo {
            name: array_name.clone(),
            packed_size: array_size,
            size: array_size,
        })),
    );
    return Ok(());
}

fn process_modifier_type(
    processed: &mut ProcessState,
    printer: &mut Printer,
    tyidx: TypeIndex,
    ty: &parser::TypeModifier,
    ty_max_size: Option<u64>,
    inputs_hash: &[parser::FileHash],
    type_db: &HashMap<parser::TypeName, TypeIndex>,
) -> io::Result<()> {
    let real_tyidx = TypeIndex {
        input_id: tyidx.input_id,
        typoff: ty.ty,
    };
    match ty.kind() {
        parser::TypeModifierKind::Const
        | parser::TypeModifierKind::Packed
        | parser::TypeModifierKind::Volatile
        | parser::TypeModifierKind::Restrict
        | parser::TypeModifierKind::Shared
        | parser::TypeModifierKind::Atomic
        | parser::TypeModifierKind::Other => {
            return handle_sym_link(
                processed,
                printer,
                tyidx,
                real_tyidx,
                ty_max_size,
                inputs_hash,
                type_db,
            );
        }
        parser::TypeModifierKind::Pointer
        | parser::TypeModifierKind::Reference
        | parser::TypeModifierKind::RvalueReference => {
            let Some(tysize) = ty.byte_size(&inputs_hash[tyidx.input_id]) else {
                warn!("process_modifier_type: unknown byte size: tyidx={:?}", tyidx);
                return Ok(());
            };
            if let Some(maxsize) = ty_max_size {
                if maxsize < tysize {
                    warn!(
                        "process_modifier_type: invalid byte size: tyidx={:?} maxsize={} size={}",
                        tyidx, maxsize, tysize
                    );
                    return Ok(());
                }
            }

            let real_tyinfo =
                get_type_info(processed, printer, real_tyidx, None, inputs_hash, type_db)?;
            let real_tyname = if let Some(tyinfo) = &real_tyinfo {
                &tyinfo.name
            } else {
                "void"
            };
            let tyname = format!("{}*", real_tyname);

            printer.add_eq_assert(&format!("sizeof({})", tyname), tysize)?;
            processed.insert(
                tyidx,
                Some(Rc::new(TypeInfo {
                    name: tyname,
                    packed_size: tysize,
                    size: tysize,
                })),
            );
        }
    }
    return Ok(());
}

// process_type ??????, ty ??????????????? processed ??????,
// processed[ty] ??? None, ???????????????????????????.
// ?????? typedef ?????????, ????????????????????? tyidx ?????????????????? TypeInfo, ???????????? Rc.
//
// ??????????????????, ty ???????????? processed ???.
fn process_type(
    processed: &mut ProcessState,
    printer: &mut Printer,
    tyidx: TypeIndex,
    ty_max_size: Option<u64>,
    inputs_hash: &[parser::FileHash],
    type_db: &HashMap<parser::TypeName, TypeIndex>,
) -> io::Result<()> {
    debug_assert!(!processed.contains_key(&tyidx));
    processed.insert(tyidx, None); // ????????????,

    let typ = parser::Type::from_offset(&inputs_hash[tyidx.input_id], tyidx.typoff);
    let Some(typ) = typ else {
        warn!("process_type: unknown type. tyidx={:?}", tyidx);
        return Ok(());
    };
    let typ = typ.as_ref();

    match typ.kind() {
        parser::TypeKind::Void
        | parser::TypeKind::Function(_)
        | parser::TypeKind::PointerToMember(_)
        | parser::TypeKind::Subrange(_)
        | parser::TypeKind::Unspecified(_) => {}
        parser::TypeKind::Base(ty) => {
            let (Some(tyname), Some(tysize)) = (ty.name(), ty.byte_size()) else {
                warn!("process_type: base type has no name. tyidx={:?}", tyidx);
                return Ok(());
            };
            processed.insert(
                tyidx,
                Some(Rc::new(TypeInfo {
                    name: tyname.to_string(),
                    packed_size: tysize,
                    size: tysize,
                })),
            );
            printer.add_eq_assert(&format!("sizeof({})", tyname), tysize)?;
        }
        parser::TypeKind::Def(ty) => {
            let real_typidx = TypeIndex {
                input_id: tyidx.input_id,
                typoff: ty.ty,
            };
            handle_sym_link(
                processed,
                printer,
                tyidx,
                real_typidx,
                ty_max_size,
                inputs_hash,
                type_db,
            )?;
        }
        parser::TypeKind::Struct(ty) => {
            return process_struct_type(
                processed,
                printer,
                tyidx,
                ty,
                ty_max_size,
                inputs_hash,
                type_db,
            );
        }
        parser::TypeKind::Union(ty) => {
            return process_union_type(
                processed,
                printer,
                tyidx,
                ty,
                ty_max_size,
                inputs_hash,
                type_db,
            );
        }
        parser::TypeKind::Enumeration(ty) => {
            return process_enum_type(
                processed,
                printer,
                tyidx,
                ty,
                ty_max_size,
                inputs_hash,
                type_db,
            );
        }
        parser::TypeKind::Array(ty) => {
            return process_array_type(
                processed,
                printer,
                tyidx,
                ty,
                ty_max_size,
                inputs_hash,
                type_db,
            );
        }
        parser::TypeKind::Modifier(ty) => {
            return process_modifier_type(
                processed,
                printer,
                tyidx,
                ty,
                ty_max_size,
                inputs_hash,
                type_db,
            );
        }
    }
    return Ok(());
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    let mut inputs = Vec::new();
    let mut inputs_hash = Vec::new();
    for input_path in &args.so_path {
        info!("load so. path={}", input_path);
        inputs.push(parser::File::parse(input_path.clone())?);
    }
    for input_path in &args.so_file_path {
        for line in read_lines(input_path)? {
            let line = line?;
            info!("load so. path={}", &line);
            inputs.push(parser::File::parse(line)?);
        }
    }
    info!("build input file hash");
    for input in &inputs {
        inputs_hash.push(parser::FileHash::new(input.file()));
    }

    info!("build type db");
    let mut dest = Vec::new();
    let mut type_db = HashMap::new();
    for (input_id, hash) in inputs_hash.iter().enumerate() {
        for (&typoff, &typ) in hash.types.iter() {
            if is_declaration(typ) {
                continue;
            }
            if let Some(typname) = parser::TypeName::try_from(typ) {
                let typidx = TypeIndex { input_id, typoff };
                if args.is_dest(&typname) {
                    dest.push(typidx);
                }
                // type_db ?????????????????????????????? so file ???????????????, ????????? anon ty
                // ????????????.
                if typ.is_anon() {
                    continue;
                }
                // typname.is_anon() may be true
                type_db.insert(typname, typidx);
            }
        }
    }

    let mut printer = Printer::try_open(&args.out_path)?;
    let mut processed = ProcessState::new();
    for dest_ty in &dest {
        process_type(
            &mut processed,
            &mut printer,
            *dest_ty,
            None,
            &inputs_hash,
            &type_db,
        )?;
    }
    printer.finish()?;
    return Ok(());
}

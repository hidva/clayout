struct ZhanyiStruct {
  long zy_bits_2bit: 2;
};

struct ZhanyiStruct2: public ZhanyiStruct {
  char ch;
};

/*
union ZhanyiUnion1 {
  char zy_union_ch;
  ZhanyiStruct2 zy_union_zs2;
};

// 还好还好, union 不能是父类.
class ZhanyiClass1: public ZhanyiUnion1 {
  char zy_class_ch;
  long zy_class_l;
};
*/

int main() {
  ZhanyiStruct2 obj;
  return 0;
}

/*
struct ZhanyiStruct
    size: 8
    members:
        0[0.2]	zy_bits_2bit: long int
        0.2[7.6]	<padding>

base long int
    size: 8
    encoding: signed

struct ZhanyiStruct2
    size: 16
    inherits: struct ZhanyiStruct
    members:
        0[8]	<inherit>: struct ZhanyiStruct
        8[1]	ch: char
        9[7]	<padding>

base char
    size: 1
    encoding: signed char

base int
    size: 4
    encoding: signed
*/
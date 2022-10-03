struct S {
    long l;
    char ch[0];
};

int main() {
    S s;
    return 0;
}

/*
struct S
    size: 8
    members:
        0[8]	l: long int
        8[??]	ch: [char]  # 此时 ArrayType count/size 均为 None.

base long int
    size: 8
    encoding: signed

# sizetype 并不是一个合法的类型名..
base sizetype
    size: 8
    encoding: unsigned

base char
    size: 1
    encoding: signed char

base int
    size: 4
    encoding: signed
*/
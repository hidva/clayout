struct S {};
struct S1 {
    long l;
    char ch;
    S s;
};
struct S2: public S1 {
    char ch;
};
int main() {
    S2 s;
    return 0;
}

/*
struct S
	size: 1

struct S1
	size: 16
	members:
		0[8]	l: long int
		8[1]	ch: char
		9[1]	s: struct S
		10[6]	<padding>

base long int
	size: 8
	encoding: signed

base char
	size: 1
	encoding: signed char

struct S2
	size: 24
	inherits: struct S1
	members:
		0[16]	<inherit>: struct S1
		16[1]	ch: char  # 这里居然没有重用 S1 空间.
		17[7]	<padding>

base int
	size: 4
	encoding: signed
*/
// another case
namespace XXX {
struct S {};
struct S1 {
    long l:32;
    char ch;
    S s;
};
struct S2: public S1 {
    char ch;
};
int main() {
    S2 s;
    return 0;
}
}

/*
struct S
	size: 1

struct S1
	size: 8
	members:
		0[4]	l: long int  # 这里显示不了是 bitfield
		4[1]	ch: char
		5[1]	s: struct S
		6[2]	<padding>

base long int
	size: 8
	encoding: signed

base char
	size: 1
	encoding: signed char

struct S2
	size: 16
	inherits: struct S1
	members:
		0[8]	<inherit>: struct S1
		8[1]	ch: char
		9[7]	<padding>

base int
	size: 4
	encoding: signed
	*/
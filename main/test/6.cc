struct S1 {
	long l;
	char ch;
};
union u {
	long u_l;
	char u_c;
	long u_b: 2;
	S1 s1;
};

struct S {
	union u s_u;
	char s_c;
};


int main() {
	S s;
	return 0;
}

/*

struct S1
	size: 16
	members:
		0[8]	l: long int
		8[1]	ch: char
		9[7]	<padding>

base long int
	size: 8
	encoding: signed

base char
	size: 1
	encoding: signed char

union u
	size: 16
	members:
		0[8]	u_l: long int
		0[1]	u_c: char
		0[0.2]	u_b: long int
		0[16]	s1: struct S1

struct S
	size: 24
	members:
		0[16]	s_u: union u
		16[1]	s_c: char
		17[7]	<padding>

base int
	size: 4
	encoding: signed
*/
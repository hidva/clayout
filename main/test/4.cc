struct S
{
    // will usually occupy 2 bytes:
    // 3 bits: value of b1
    // 5 bits: unused
    // 2 bits: value of b2
    // 6 bits: unused
    unsigned long b1 : 3;
    unsigned char :0; // start a new byte
    unsigned short b2 : 2;
};

int main()
{
	S s;
	return sizeof(S);
}
/*
struct S
	size: 8
	members:
		0[0.3]	b1: long unsigned int
		0.3[0.5]	<padding>
		1[0.2]	b2: short unsigned int
		1.2[6.6]	<padding>

base long unsigned int
	size: 8
	encoding: unsigned

base short unsigned int
	size: 2
	encoding: unsigned

base int
	size: 4
	encoding: signed
*/
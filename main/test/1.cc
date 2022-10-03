struct zhanyi_struct {
    union {
        long zhanyi_union_field_long;
        char zhanyi_union_field_char;
    };
    long zhanyi_s_field_long;
    char zhanyi_s_field_ch;
};

int main () {
    zhanyi_struct s;
    return 0;
}

/*
struct zhanyi_struct
    size: 24
    members:
        0[8]	<anon>: union zhanyi_struct::<anon>
            0[8]	zhanyi_union_field_long: long int
            0[1]	zhanyi_union_field_char: char
        8[8]	zhanyi_s_field_long: long int
        16[1]	zhanyi_s_field_ch: char
        17[7]	<padding>

base long int
    size: 8
    encoding: signed

base char
    size: 1
    encoding: signed char

base int
    size: 4
    encoding: signed
*/
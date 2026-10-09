/* liblzf-shaped tile decode. No Python headers, so it builds without python3-dev. */

#if defined(_WIN32)
__declspec(dllexport)
#endif
int lzf_decode(const unsigned char *src, int src_len, unsigned char *out, int max_out) {
    int ip = 0;
    int op = 0;
    if (!src || !out || src_len < 0 || max_out < 0) return -1;
    while (ip < src_len && op < max_out) {
        unsigned char bite = src[ip];
        int ctrl = (int)bite + 1;
        int ofs = (bite & 31) << 8;
        int length = bite >> 5;
        ip += 1;
        if (ctrl < 33) {
            int take;
            if (ip + ctrl > src_len) return -1;
            take = ctrl;
            if (take > max_out - op) take = max_out - op;
            for (int i = 0; i < take; i++) out[op + i] = src[ip + i];
            op += take;
            ip += ctrl;
        } else {
            int ref;
            int count;
            int i;
            length -= 1;
            if (length == 6) {
                if (ip >= src_len) return -1;
                length += src[ip];
                ip += 1;
            }
            if (ip >= src_len) return -1;
            ref = op - ofs - 1 - src[ip];
            ip += 1;
            count = length + 3;
            if (ref < 0) return -1;
            for (i = 0; i < count && op < max_out; i++) {
                if (ref < 0 || ref >= op) return -1;
                out[op] = out[ref];
                op += 1;
                ref += 1;
            }
        }
    }
    return op >= max_out ? op : -1;
}

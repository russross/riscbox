#define USE_BUILTIN_CRYPTO
#include "fs_wget.c"

static uint8_t output[64];
static size_t output_len;

static int collect(void *opaque, const uint8_t *data, size_t len)
{
    (void)opaque;
    if (output_len + len > sizeof(output))
        return -1;
    memcpy(output + output_len, data, len);
    output_len += len;
    return len;
}

static void check(int condition)
{
    if (!condition)
        abort();
}

int main(void)
{
    static const uint8_t expected_key[32] = {
        0xae, 0x4d, 0x0c, 0x95, 0xaf, 0x6b, 0x46, 0xd3,
        0x2d, 0x0a, 0xdf, 0xf9, 0x28, 0xf0, 0x6d, 0xd0,
        0x2a, 0x30, 0x3f, 0x8e, 0xf3, 0xc2, 0x51, 0xdf,
        0xd6, 0xe2, 0xd8, 0x5a, 0x95, 0x47, 0x4c, 0x43,
    };
    static const uint8_t ciphertext[32] = {
        0x46, 0x69, 0xc3, 0xaf, 0x35, 0x8f, 0x11, 0xd4,
        0xa3, 0xff, 0x5a, 0x2b, 0xcc, 0xde, 0xfe, 0x2e,
        0xa2, 0xa8, 0x67, 0xc9, 0xe1, 0x5d, 0xf0, 0x23,
        0xc3, 0x8e, 0x97, 0x13, 0x7d, 0x80, 0xa0, 0xad,
    };
    static const uint8_t plaintext[] = "browser filesystem payload";
    uint8_t derived[32];
    uint8_t file[4 + 16 + sizeof(ciphertext)];
    uint8_t key_bytes[16];
    AESDecryptKey key;
    DecryptFileState *state;

    pbkdf2_hmac_sha256((const uint8_t *)"password", 8,
                       (const uint8_t *)"salt", 4, 2, sizeof(derived), derived);
    check(memcmp(derived, expected_key, sizeof(derived)) == 0);

    memset(key_bytes, 0x31, sizeof(key_bytes));
    aes_decrypt_key_init(&key, key_bytes);
    memcpy(file, encrypted_file_magic, 4);
    memset(file + 4, 0x72, 16);
    memcpy(file + 20, ciphertext, sizeof(ciphertext));
    state = decrypt_file_init(&key, collect, NULL);
    check(decrypt_file(state, file, 7) == 0);
    check(decrypt_file(state, file + 7, sizeof(file) - 7) == 0);
    check(decrypt_file_flush(state) == 0);
    check(output_len == sizeof(plaintext) - 1);
    check(memcmp(output, plaintext, output_len) == 0);
    decrypt_file_end(state);

    file[0] = 0;
    state = decrypt_file_init(&key, collect, NULL);
    check(decrypt_file(state, file, sizeof(file)) < 0);
    decrypt_file_end(state);
    return 0;
}

#define _WIN32_WINNT 0x0601
#include <windows.h>
#include <stdio.h>
#include <wchar.h>

int wmain(int argc, wchar_t **argv)
{
    static const wchar_t chinese[] = L"\x6b22\x8fce\x4f7f\x7528 MUI";
    static const wchar_t english[] = L"Welcome to MUI";
    ULONG count = 0;
    wchar_t text[64];
    if (argc != 2 && argc != 3)
        return 1;
    BOOL missing = argc == 3 && wcscmp(argv[2], L"--missing") == 0;
    BOOL use_english = argc == 3 && wcscmp(argv[2], L"--en-US") == 0;
    if (argc == 3 && !missing && !use_english)
        return 1;
    if (!SetThreadPreferredUILanguages(MUI_LANGUAGE_NAME,
        use_english ? L"en-US\0" : L"zh-CN\0", &count) || count == 0) {
        fprintf(stderr, "SetThreadPreferredUILanguages failed (%lu)\n", GetLastError());
        return 2;
    }
    HMODULE module = LoadLibraryExW(argv[1], NULL,
        LOAD_LIBRARY_AS_IMAGE_RESOURCE | LOAD_LIBRARY_AS_DATAFILE);
    if (module == NULL) {
        fprintf(stderr, "LoadLibraryExW failed (%lu)\n", GetLastError());
        return 3;
    }
    int length = LoadStringW(module, 101, text, ARRAYSIZE(text));
    FreeLibrary(module);
    if (missing) {
        if (length == 0) {
            puts("PASS absent satellite has no string 101");
            return 0;
        }
        fprintf(stderr, "String 101 exists with the satellite absent\n");
        return 4;
    }
    const wchar_t *expected = use_english ? english : chinese;
    const char *language = use_english ? "en-US" : "zh-CN";
    if (length != wcslen(expected) || wcscmp(text, expected) != 0) {
        fprintf(stderr, "LoadStringW did not return the expected %s string\n", language);
        return 5;
    }
    printf("PASS main DLL loaded %s string 101\n", language);
    return 0;
}

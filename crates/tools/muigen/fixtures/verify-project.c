#define _WIN32_WINNT 0x0601
#include <windows.h>

static BOOL check_string(HMODULE module, const WCHAR *languages,
                         const WCHAR *expected, int length)
{
    ULONG count = 0;
    WCHAR text[64];
    if (!SetThreadPreferredUILanguages(MUI_LANGUAGE_NAME, languages, &count) || count != 1)
        return FALSE;
    if (LoadStringW(module, 101, text, ARRAYSIZE(text)) != length)
        return FALSE;
    if (length == 0)
        return TRUE;
    for (int index = 0; index <= length; ++index) {
        if (text[index] != expected[index])
            return FALSE;
    }
    return TRUE;
}

void WINAPI mui_probe(void)
{
    static const WCHAR chinese[] = L"\x6b22\x8fce\x4f7f\x7528 MUI";
    static const WCHAR english[] = L"Welcome to MUI";
    HMODULE absent = LoadLibraryW(L"C:\\mui-absent\\Sample.dll");
    if (!absent)
        ExitProcess(10);
    if (!check_string(absent, L"zh-CN\0", NULL, 0))
        ExitProcess(11);
    if (!FreeLibrary(absent))
        ExitProcess(12);

    HMODULE batch = LoadLibraryW(L"C:\\mui-batch\\Sample.dll");
    if (!batch)
        ExitProcess(20);
    if (!check_string(batch, L"zh-CN\0", chinese, ARRAYSIZE(chinese) - 1))
        ExitProcess(21);
    if (!check_string(batch, L"en-US\0", english, ARRAYSIZE(english) - 1))
        ExitProcess(22);
    if (!FreeLibrary(batch))
        ExitProcess(23);

    HMODULE built = LoadLibraryW(L"C:\\mui-built\\Sample.dll");
    if (!built)
        ExitProcess(30);
    if (!check_string(built, L"zh-CN\0", chinese, ARRAYSIZE(chinese) - 1))
        ExitProcess(31);
    if (!FreeLibrary(built))
        ExitProcess(32);
    ExitProcess(0);
}

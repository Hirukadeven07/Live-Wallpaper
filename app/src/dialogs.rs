// Windows' own file pickers and message boxes, opening folders and links, and the Start-with-Windows switch.

use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::HWND,
        System::{
            Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
            Registry::{
                RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
                KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
            },
        },
        UI::{
            Shell::{
                Common::COMDLG_FILTERSPEC, FileOpenDialog, IFileOpenDialog, ShellExecuteW, FOS_ALLOWMULTISELECT,
                FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

/// Ask for files (or one folder). An empty list if the user cancels.
pub fn pick(owner: Option<HWND>, title: &str, filters: &[(&str, &str)], multi: bool, folder: bool) -> Vec<String> {
    unsafe {
        let Ok(dlg) = CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) else {
            return Vec::new();
        };
        let mut opts = dlg.GetOptions().unwrap_or_default() | FOS_FORCEFILESYSTEM;
        if folder {
            opts |= FOS_PICKFOLDERS;
        } else {
            opts |= FOS_FILEMUSTEXIST;
            if multi {
                opts |= FOS_ALLOWMULTISELECT;
            }
        }
        let _ = dlg.SetOptions(opts);
        let _ = dlg.SetTitle(&HSTRING::from(title));
        // The strings have to outlive the call that uses them.
        let names: Vec<HSTRING> = filters.iter().map(|f| HSTRING::from(f.0)).collect();
        let specs_s: Vec<HSTRING> = filters.iter().map(|f| HSTRING::from(f.1)).collect();
        let specs: Vec<COMDLG_FILTERSPEC> = names
            .iter()
            .zip(specs_s.iter())
            .map(|(n, s)| COMDLG_FILTERSPEC { pszName: PCWSTR(n.as_ptr()), pszSpec: PCWSTR(s.as_ptr()) })
            .collect();
        if !folder && !specs.is_empty() {
            let _ = dlg.SetFileTypes(&specs);
        }
        if dlg.Show(owner).is_err() {
            return Vec::new(); // cancelled
        }
        let mut out = Vec::new();
        if let Ok(items) = dlg.GetResults() {
            let n = items.GetCount().unwrap_or(0);
            for i in 0..n {
                if let Ok(item) = items.GetItemAt(i) {
                    if let Ok(p) = item.GetDisplayName(SIGDN_FILESYSPATH) {
                        out.push(p.to_string().unwrap_or_default());
                        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
                    }
                }
            }
        }
        out
    }
}

/// Open a folder in File Explorer, or a link in the browser.
pub fn open(target: &str) {
    unsafe {
        ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

// ---------- Start with Windows ----------
// A value under HKCU\...\Run, like most apps. It points at wherever this exe is now.

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "LiveWallpaper";

fn command() -> String {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    format!("\"{exe}\" --background")
}

pub fn autostart_current() -> Option<String> {
    unsafe {
        let mut buf = vec![0u16; 2048];
        let mut size = (buf.len() * 2) as u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN_KEY),
            &HSTRING::from(RUN_NAME),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        r.is_ok().then(|| crate::util::from_wide(&buf))
    }
}

pub fn autostart_enabled() -> bool {
    autostart_current().is_some()
}

pub fn set_autostart(on: bool) {
    if crate::util::test_data_dir().is_some() {
        return; // test runs never change the real entry
    }
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN_KEY),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .is_err()
        {
            crate::log!("couldn't open the Run key to change Start with Windows");
            return;
        }
        if on {
            let v = crate::util::wide(&command());
            let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
            let _ = RegSetValueExW(key, &HSTRING::from(RUN_NAME), None, REG_SZ, Some(bytes));
        } else {
            let _ = RegDeleteValueW(key, &HSTRING::from(RUN_NAME));
        }
        let _ = RegCloseKey(key);
    }
}

/// If Start with Windows is on but points at an older copy of the exe, point it here.
pub fn refresh_autostart() {
    if let Some(cur) = autostart_current() {
        if cur != command() {
            set_autostart(true);
        }
    }
}

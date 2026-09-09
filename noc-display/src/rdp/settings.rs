use super::{
    dispatch::{get, put, Value},
    interfaces::*,
};
use crate::config::RdpSettings;
use windows::{
    core::*,
    Win32::{Foundation::*, System::Com::*},
};

pub fn configure(
    control: &IDispatch,
    config: &RdpSettings,
    width: i32,
    height: i32,
    parent: HWND,
) -> Result<()> {
    let ns: IMsRdpClientNonScriptable5 = control.cast()?;
    let ns2: IMsRdpClientNonScriptable2 = control.cast()?;
    let ns3: IMsRdpClientNonScriptable3 = control.cast()?;
    let ns4: IMsRdpClientNonScriptable4 = control.cast()?;
    unsafe {
        // Mandatory safety capabilities, available since Windows 7. Fail closed.
        ns2.put_UIParentWindowHandle(parent).ok()?;
        ns.put_AllowPromptingForCredentials(0).ok()?;
        ns3.put_PromptForCredentials(0).ok()?;
        ns4.put_PromptForCredsOnClient(0).ok()?;
        ns4.put_AllowCredentialSaving(0).ok()?;
        ns3.put_NegotiateSecurityLayer(-1).ok()?;
        ns.put_DisableConnectionBar(-1).ok()?;
        ns.put_UseMultimon(0).ok()?;
        ns3.put_RedirectDynamicDevices(0).ok()?;
        ns3.put_RedirectDynamicDrives(0).ok()?;
    }
    put(control, "Server", Value::string(&config.server))?;
    put(control, "UserName", Value::string(&config.username))?;
    put(control, "Domain", Value::string(""))?;
    put(control, "DesktopWidth", Value::int(width))?;
    put(control, "DesktopHeight", Value::int(height))?;
    put(control, "ColorDepth", Value::int(32))?;
    put(control, "FullScreen", Value::boolean(false))?;
    let advanced = get(control, "AdvancedSettings8")
        .or_else(|_| get(control, "AdvancedSettings7"))?
        .dispatch()?;
    put(&advanced, "RDPPort", Value::int(config.port as i32))?;
    // Explicit per-user override: 0 skips server authentication; 2 would prompt.
    let authentication_level = if config.ignore_certificate_errors {
        0
    } else {
        1
    };
    put(
        &advanced,
        "AuthenticationLevel",
        Value::int(authentication_level),
    )?;
    put(&advanced, "EnableAutoReconnect", Value::boolean(false))?;
    put(&advanced, "DisplayConnectionBar", Value::boolean(false))?;
    put(&advanced, "GrabFocusOnConnect", Value::boolean(false))?;
    put(&advanced, "RedirectDrives", Value::boolean(false))?;
    put(&advanced, "RedirectPrinters", Value::boolean(false))?;
    put(&advanced, "RedirectPorts", Value::boolean(false))?;
    put(&advanced, "RedirectSmartCards", Value::boolean(false))?;
    put(&advanced, "RedirectClipboard", Value::boolean(false))?;
    // Bounded transport establishment; the app also enforces a logon watchdog.
    put(&advanced, "singleConnectionTimeout", Value::int(20))?;
    put(&advanced, "overallConnectionTimeout", Value::int(30))?;
    // Check the installed control actually retained the noninteractive security policy.
    unsafe {
        let mut prompt = -1i16;
        ns.get_AllowPromptingForCredentials(&mut prompt).ok()?;
        let auth = get(&advanced, "AuthenticationLevel")?;
        let value = &auth.0.Anonymous.Anonymous;
        use windows::Win32::System::Variant::{VT_I4, VT_UI4};
        if prompt != 0
            || !matches!(value.vt, VT_I4 | VT_UI4)
            || value.Anonymous.ulVal != authentication_level as u32
        {
            return Err(Error::new(
                E_ACCESSDENIED,
                "RDP security policy was not retained".into(),
            ));
        }
    }
    if config.ignore_certificate_errors {
        crate::log_error("RDP certificate validation disabled by config (AuthenticationLevel=0)");
    }
    // Optional comfort property: lack of support must not compromise hosting.
    if let Err(e) = put(&advanced, "SmartSizing", Value::boolean(false)) {
        crate::log_error(format!(
            "RDP optional SmartSizing HRESULT={:08x}",
            e.code().0
        ));
    }
    Ok(())
}

/// Must run after ClearTextPassword and before Connect for xRDP initial autologon.
pub fn finalize_xrdp_credentials(control: &IDispatch) -> Result<()> {
    // AdvancedSettings7 returns IMsRdpClientAdvancedSettings6 (not version 7).
    let advanced6 = get(control, "AdvancedSettings7")?.dispatch()?;
    put(&advanced6, "EnableCredSspSupport", Value::boolean(false))?;
    unsafe {
        use windows::Win32::System::Variant::VT_BOOL;
        let retained = get(&advanced6, "EnableCredSspSupport")?;
        let value = &retained.0.Anonymous.Anonymous;
        if value.vt != VT_BOOL || value.Anonymous.boolVal.0 != 0 {
            return Err(E_ACCESSDENIED.into());
        }
    }
    crate::log_error(
        "RDP xRDP credentials supplied before Connect; Domain empty; CredSSP disabled",
    );
    Ok(())
}

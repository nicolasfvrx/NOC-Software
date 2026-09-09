//! Typed, inherited COM ABI from the MsTscAx type library (interfaces available by Windows 7).
#![allow(dead_code)]
use windows::{
    core::*,
    Win32::{Foundation::HWND, System::Variant::VARIANT},
};
#[interface("c1e6743a-41c1-4a74-832a-0dd06c1c7a0e")]
pub unsafe trait IMsTscNonScriptable: IUnknown {
    pub fn put_ClearTextPassword(&self, _arg1: *const u16) -> HRESULT;
    pub fn put_PortablePassword(&self, pPortablePass: *const u16) -> HRESULT;
    pub fn get_PortablePassword(&self, pPortablePass: *mut *const u16) -> HRESULT;
    pub fn put_PortableSalt(&self, pPortableSalt: *const u16) -> HRESULT;
    pub fn get_PortableSalt(&self, pPortableSalt: *mut *const u16) -> HRESULT;
    pub fn put_BinaryPassword(&self, pBinaryPassword: *const u16) -> HRESULT;
    pub fn get_BinaryPassword(&self, pBinaryPassword: *mut *const u16) -> HRESULT;
    pub fn put_BinarySalt(&self, pSalt: *const u16) -> HRESULT;
    pub fn get_BinarySalt(&self, pSalt: *mut *const u16) -> HRESULT;
    pub fn ResetPassword(&self) -> HRESULT;
}
#[interface("2f079c4c-87b2-4afd-97ab-20cdb43038ae")]
pub unsafe trait IMsRdpClientNonScriptable: IMsTscNonScriptable {
    pub fn NotifyRedirectDeviceChange(&self, wParam: usize, lParam: isize) -> HRESULT;
    pub fn SendKeys(&self, numKeys: i32, pbArrayKeyUp: *mut i16, plKeyData: *mut i32) -> HRESULT;
}
#[interface("17a5e535-4072-4fa4-af32-c8d0d47345e9")]
pub unsafe trait IMsRdpClientNonScriptable2: IMsRdpClientNonScriptable {
    pub fn put_UIParentWindowHandle(&self, phwndUIParentWindowHandle: HWND) -> HRESULT;
    pub fn get_UIParentWindowHandle(&self, phwndUIParentWindowHandle: *mut HWND) -> HRESULT;
}
#[interface("b3378d90-0728-45c7-8ed7-b6159fb92219")]
pub unsafe trait IMsRdpClientNonScriptable3: IMsRdpClientNonScriptable2 {
    pub fn put_ShowRedirectionWarningDialog(&self, pfShowRdrDlg: i16) -> HRESULT;
    pub fn get_ShowRedirectionWarningDialog(&self, pfShowRdrDlg: *mut i16) -> HRESULT;
    pub fn put_PromptForCredentials(&self, pfPrompt: i16) -> HRESULT;
    pub fn get_PromptForCredentials(&self, pfPrompt: *mut i16) -> HRESULT;
    pub fn put_NegotiateSecurityLayer(&self, pfNegotiate: i16) -> HRESULT;
    pub fn get_NegotiateSecurityLayer(&self, pfNegotiate: *mut i16) -> HRESULT;
    pub fn put_EnableCredSspSupport(&self, pfEnableSupport: i16) -> HRESULT;
    pub fn get_EnableCredSspSupport(&self, pfEnableSupport: *mut i16) -> HRESULT;
    pub fn put_RedirectDynamicDrives(&self, pfRedirectDynamicDrives: i16) -> HRESULT;
    pub fn get_RedirectDynamicDrives(&self, pfRedirectDynamicDrives: *mut i16) -> HRESULT;
    pub fn put_RedirectDynamicDevices(&self, pfRedirectDynamicDevices: i16) -> HRESULT;
    pub fn get_RedirectDynamicDevices(&self, pfRedirectDynamicDevices: *mut i16) -> HRESULT;
    pub fn get_DeviceCollection(&self, ppDeviceCollection: *mut *mut core::ffi::c_void) -> HRESULT;
    pub fn get_DriveCollection(&self, ppDeviceCollection: *mut *mut core::ffi::c_void) -> HRESULT;
    pub fn put_WarnAboutSendingCredentials(&self, pfWarn: i16) -> HRESULT;
    pub fn get_WarnAboutSendingCredentials(&self, pfWarn: *mut i16) -> HRESULT;
    pub fn put_WarnAboutClipboardRedirection(&self, pfWarn: i16) -> HRESULT;
    pub fn get_WarnAboutClipboardRedirection(&self, pfWarn: *mut i16) -> HRESULT;
    pub fn put_ConnectionBarText(&self, pConnectionBarText: *const u16) -> HRESULT;
    pub fn get_ConnectionBarText(&self, pConnectionBarText: *mut *const u16) -> HRESULT;
}
#[interface("f50fa8aa-1c7d-4f59-b15c-a90cacae1fcb")]
pub unsafe trait IMsRdpClientNonScriptable4: IMsRdpClientNonScriptable3 {
    pub fn put_RedirectionWarningType(&self, pWrnType: i32) -> HRESULT;
    pub fn get_RedirectionWarningType(&self, pWrnType: *mut i32) -> HRESULT;
    pub fn put_MarkRdpSettingsSecure(&self, pfRdpSecure: i16) -> HRESULT;
    pub fn get_MarkRdpSettingsSecure(&self, pfRdpSecure: *mut i16) -> HRESULT;
    pub fn put_PublisherCertificateChain(&self, pVarCert: *mut VARIANT) -> HRESULT;
    pub fn get_PublisherCertificateChain(&self, pVarCert: *mut VARIANT) -> HRESULT;
    pub fn put_WarnAboutPrinterRedirection(&self, pfWarn: i16) -> HRESULT;
    pub fn get_WarnAboutPrinterRedirection(&self, pfWarn: *mut i16) -> HRESULT;
    pub fn put_AllowCredentialSaving(&self, pfAllowSave: i16) -> HRESULT;
    pub fn get_AllowCredentialSaving(&self, pfAllowSave: *mut i16) -> HRESULT;
    pub fn put_PromptForCredsOnClient(&self, pfPromptForCredsOnClient: i16) -> HRESULT;
    pub fn get_PromptForCredsOnClient(&self, pfPromptForCredsOnClient: *mut i16) -> HRESULT;
    pub fn put_LaunchedViaClientShellInterface(
        &self,
        pfLaunchedViaClientShellInterface: i16,
    ) -> HRESULT;
    pub fn get_LaunchedViaClientShellInterface(
        &self,
        pfLaunchedViaClientShellInterface: *mut i16,
    ) -> HRESULT;
    pub fn put_TrustedZoneSite(&self, pfIsTrustedZone: i16) -> HRESULT;
    pub fn get_TrustedZoneSite(&self, pfIsTrustedZone: *mut i16) -> HRESULT;
}
#[interface("4f6996d5-d7b1-412c-b0ff-063718566907")]
pub unsafe trait IMsRdpClientNonScriptable5: IMsRdpClientNonScriptable4 {
    pub fn put_UseMultimon(&self, pfUseMultimon: i16) -> HRESULT;
    pub fn get_UseMultimon(&self, pfUseMultimon: *mut i16) -> HRESULT;
    pub fn get_RemoteMonitorCount(&self, pcRemoteMonitors: *mut u32) -> HRESULT;
    pub fn GetRemoteMonitorsBoundingBox(
        &self,
        pLeft: *mut i32,
        pTop: *mut i32,
        pRight: *mut i32,
        pBottom: *mut i32,
    ) -> HRESULT;
    pub fn get_RemoteMonitorLayoutMatchesLocal(&self, pfRemoteMatchesLocal: *mut i16) -> HRESULT;
    pub fn put_DisableConnectionBar(&self, _arg1: i16) -> HRESULT;
    pub fn put_DisableRemoteAppCapsCheck(&self, pfDisableRemoteAppCapsCheck: i16) -> HRESULT;
    pub fn get_DisableRemoteAppCapsCheck(&self, pfDisableRemoteAppCapsCheck: *mut i16) -> HRESULT;
    pub fn put_WarnAboutDirectXRedirection(&self, pfWarn: i16) -> HRESULT;
    pub fn get_WarnAboutDirectXRedirection(&self, pfWarn: *mut i16) -> HRESULT;
    pub fn put_AllowPromptingForCredentials(&self, pfAllow: i16) -> HRESULT;
    pub fn get_AllowPromptingForCredentials(&self, pfAllow: *mut i16) -> HRESULT;
}

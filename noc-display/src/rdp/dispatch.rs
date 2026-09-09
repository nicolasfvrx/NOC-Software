//! Owning VARIANT wrapper. No borrowed BSTR/interface escapes an Invoke call.
use std::mem::ManuallyDrop;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, Variant::*},
    },
};

pub struct Value(pub VARIANT);
impl Drop for Value {
    fn drop(&mut self) {
        unsafe {
            let _ = VariantClear(&mut self.0);
        }
    }
}
impl Value {
    pub fn empty() -> Self {
        Self(VARIANT::default())
    }
    pub fn int(value: i32) -> Self {
        let mut v = Self::empty();
        unsafe {
            (*v.0.Anonymous.Anonymous).vt = VT_I4;
            (*v.0.Anonymous.Anonymous).Anonymous.lVal = value;
        }
        v
    }
    pub fn boolean(value: bool) -> Self {
        let mut v = Self::empty();
        unsafe {
            (*v.0.Anonymous.Anonymous).vt = VT_BOOL;
            (*v.0.Anonymous.Anonymous).Anonymous.boolVal = VARIANT_BOOL(if value { -1 } else { 0 });
        }
        v
    }
    pub fn string(value: &str) -> Self {
        let mut v = Self::empty();
        unsafe {
            (*v.0.Anonymous.Anonymous).vt = VT_BSTR;
            (*v.0.Anonymous.Anonymous).Anonymous.bstrVal = ManuallyDrop::new(BSTR::from(value));
        }
        v
    }
    pub fn dispatch(&self) -> Result<IDispatch> {
        unsafe {
            if self.0.Anonymous.Anonymous.vt != VT_DISPATCH {
                return Err(E_NOINTERFACE.into());
            }
            self.0
                .Anonymous
                .Anonymous
                .Anonymous
                .pdispVal
                .as_ref()
                .cloned()
                .ok_or_else(|| E_POINTER.into())
        }
    }
    pub fn into_raw(mut self) -> VARIANT {
        std::mem::take(&mut self.0)
    }
}

pub fn invoke(
    object: &IDispatch,
    name: &str,
    flags: DISPATCH_FLAGS,
    value: Option<&mut Value>,
) -> Result<Value> {
    unsafe {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut id = 0;
        object.GetIDsOfNames(&GUID::zeroed(), &PCWSTR(wide.as_ptr()), 1, 0, &mut id)?;
        let mut named = -3i32; // DISPID_PROPERTYPUT
        let mut params = DISPPARAMS::default();
        if let Some(value) = value {
            params.rgvarg = &mut value.0;
            params.cArgs = 1;
            params.rgdispidNamedArgs = &mut named;
            params.cNamedArgs = 1;
        }
        let mut result = Value::empty();
        // Do not request or log exception strings: an ActiveX server may include input values.
        object.Invoke(
            id,
            &GUID::zeroed(),
            0,
            flags,
            &params,
            Some(&mut result.0),
            None,
            None,
        )?;
        Ok(result)
    }
}
pub fn put(object: &IDispatch, name: &str, mut value: Value) -> Result<()> {
    invoke(object, name, DISPATCH_PROPERTYPUT, Some(&mut value))
        .map(|_| ())
        .map_err(|e| {
            Error::new(
                e.code(),
                format!("RDP property {name} HRESULT={:08x}", e.code().0).into(),
            )
        })
}
pub fn get(object: &IDispatch, name: &str) -> Result<Value> {
    invoke(object, name, DISPATCH_PROPERTYGET, None)
}
pub fn call(object: &IDispatch, name: &str) -> Result<()> {
    invoke(object, name, DISPATCH_METHOD, None).map(|_| ())
}

/// MsTscAx's local error lookup takes two numeric inputs, never credentials.
pub fn error_description(object: &IDispatch, reason: i32, extended: i32) -> Result<String> {
    unsafe {
        let mut id = 0;
        object.GetIDsOfNames(&GUID::zeroed(), &w!("GetErrorDescription"), 1, 0, &mut id)?;
        // IDispatch arguments are in reverse order. These VARIANTs own no allocations.
        let mut args = [
            Value::int(extended).into_raw(),
            Value::int(reason).into_raw(),
        ];
        let params = DISPPARAMS {
            rgvarg: args.as_mut_ptr(),
            cArgs: 2,
            ..Default::default()
        };
        let mut result = Value::empty();
        object.Invoke(
            id,
            &GUID::zeroed(),
            0,
            DISPATCH_METHOD,
            &params,
            Some(&mut result.0),
            None,
            None,
        )?;
        let data = &result.0.Anonymous.Anonymous;
        if data.vt != VT_BSTR {
            return Err(DISP_E_TYPEMISMATCH.into());
        }
        Ok(data
            .Anonymous
            .bstrVal
            .to_string()
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .take(2048)
            .collect())
    }
}

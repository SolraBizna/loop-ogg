use std::{
    env,
};

fn is_utf8(var_name: &str) -> Option<bool> {
    match env::var(var_name) {
	Err(_) => None,
	Ok(x) => {
	    let mut it = x.split('.');
	    let _lang = it.next();
	    let charset = it.next();
	    match charset {
		None => None,
		Some(x) => {
		    // there might be a "@modifier"
		    let mut it = x.split('@');
		    let charset = it.next().unwrap(); // should always succeed
		    let charset = charset.to_ascii_lowercase();
		    Some(charset == "utf-8")
		},
	    }
	},
    }
}

/// Returns a best guess as to whether Unicode codes can be used on our
/// terminal.
pub fn am_unicode() -> bool {
    if cfg!(target_os = "windows") {
	// TODO?
	false
    }
    else {
	is_utf8("LC_ALL")
	    .or_else(|| is_utf8("LC_CTYPE"))
	    .or_else(|| is_utf8("LANG"))
	    .unwrap_or(false)
    }
}
    

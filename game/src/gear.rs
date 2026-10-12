//! Which model draws each weapon. Every gun is meant to have its own: `models/<key>.glb` on a body, on the floor and in
//! previews, and `models/vm-<key>.glb` in first person. Both are built by `tools/models/build.py` (see `guns.py`) from
//! a model of that very weapon, put in the hands of one of the animated packs. Until a gun has its own, it is drawn
//! with its class's shared model: the M4, the AK-74M, the bolt-action rifle, and so on (see `actors::prop_kind` and
//! `viewmodel::family_of`).

use zugriff_sim::weapons::WeaponId;

use crate::actors::{prop_file, prop_kind};

/// A gun's own model files: the viewmodel, the prop, and, aimed, the eye's distance behind the rear sight (m).
/// Without sights (`None`): aimed, it keeps the crosshair, and the scoped rifles use the scope overlay.
pub struct Own {
    pub vm: &'static str,
    pub prop: &'static str,
    pub eye: Option<f32>,
}

/// Guns aimed through an optic, not iron sights: the G36's carry handle holds a scope whose lens the model draws solid
/// (its backup sights sit behind it, on the same line), so aimed it looks through the scope overlay, the rig hidden,
/// as a scoped rifle does (with the ADS zoom only).
pub fn optic(w: WeaponId) -> bool {
    w == WeaponId::G36
}

/// Whether this inventory's gun is aimed far enough that the eye is at its optic.
pub fn in_optic(inv: &zugriff_sim::weapons::Inventory) -> bool {
    optic(inv.active) && inv.ads > 0.85
}

/// The weapons that have their own model files.
pub fn own(w: WeaponId) -> Option<&'static Own> {
    OWN.iter().find(|(x, _)| *x == w).map(|(_, o)| o)
}

/// (The G95 is an HK416, the G22 an AWM, the P8 a USP, the PP-19 a Bizon, the M1014 a Benelli M4; the MG5 is drawn as its
/// forerunner the HK21E and the P30 as a VP9, for want of models of them.)
const OWN: &[(WeaponId, Own)] = &[
    (WeaponId::G36, Own { vm: "models/vm-g36.glb", prop: "models/g36.glb", eye: Some(0.12) }),
    (WeaponId::G95, Own { vm: "models/vm-hk416.glb", prop: "models/hk416.glb", eye: Some(0.12) }),
    (WeaponId::Akm, Own { vm: "models/vm-akm.glb", prop: "models/akm.glb", eye: Some(0.25) }),
    (WeaponId::G22, Own { vm: "models/vm-awm.glb", prop: "models/awm.glb", eye: None }),
    (WeaponId::G28, Own { vm: "models/vm-g28.glb", prop: "models/g28.glb", eye: None }),
    (WeaponId::Svd, Own { vm: "models/vm-svd.glb", prop: "models/svd.glb", eye: None }),
    (WeaponId::Mg5, Own { vm: "models/vm-mg5.glb", prop: "models/mg5.glb", eye: Some(0.14) }),
    (WeaponId::Pkm, Own { vm: "models/vm-pkm.glb", prop: "models/pkm.glb", eye: Some(0.3) }),
    (WeaponId::Mp7, Own { vm: "models/vm-mp7.glb", prop: "models/mp7.glb", eye: Some(0.15) }),
    (WeaponId::Pp19, Own { vm: "models/vm-bizon.glb", prop: "models/bizon.glb", eye: Some(0.15) }),
    (WeaponId::Mp5, Own { vm: "models/vm-mp5.glb", prop: "models/mp5.glb", eye: Some(0.15) }),
    (WeaponId::Ump, Own { vm: "models/vm-ump.glb", prop: "models/ump.glb", eye: Some(0.15) }),
    (WeaponId::P8, Own { vm: "models/vm-usp.glb", prop: "models/usp.glb", eye: Some(0.2) }),
    (WeaponId::Mp443, Own { vm: "models/vm-mp443.glb", prop: "models/mp443.glb", eye: Some(0.2) }),
    (WeaponId::P30, Own { vm: "models/vm-p30.glb", prop: "models/p30.glb", eye: Some(0.2) }),
    (WeaponId::Rev357, Own { vm: "models/vm-revolver.glb", prop: "models/revolver.glb", eye: Some(0.2) }),
    (WeaponId::M1014, Own { vm: "models/vm-benelli.glb", prop: "models/benelli.glb", eye: Some(0.2) }),
];

/// The model of a weapon on its own (on a body, on the floor, in a preview).
pub fn prop_path(w: WeaponId) -> &'static str {
    match own(w) {
        Some(o) => o.prop,
        None => prop_file(prop_kind(w)).0,
    }
}

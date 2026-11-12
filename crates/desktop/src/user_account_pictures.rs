pub struct UserAccountPicture {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

pub const USER: UserAccountPicture = UserAccountPicture {
    name: "user.bmp",
    bytes: include_bytes!("../assets/windows7/User Account Pictures/user.bmp"),
};

pub const GUEST: UserAccountPicture = UserAccountPicture {
    name: "guest.bmp",
    bytes: include_bytes!("../assets/windows7/User Account Pictures/guest.bmp"),
};

pub const DEFAULT_PICTURES: [UserAccountPicture; 35] = [
    UserAccountPicture {
        name: "usertile10.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile10.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile11.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile11.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile12.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile12.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile13.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile13.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile14.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile14.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile15.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile15.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile16.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile16.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile17.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile17.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile18.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile18.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile19.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile19.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile20.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile20.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile21.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile21.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile22.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile22.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile23.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile23.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile24.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile24.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile25.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile25.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile26.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile26.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile27.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile27.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile28.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile28.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile29.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile29.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile30.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile30.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile31.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile31.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile32.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile32.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile33.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile33.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile34.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile34.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile35.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile35.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile36.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile36.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile37.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile37.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile38.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile38.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile39.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile39.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile40.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile40.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile41.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile41.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile42.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile42.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile43.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile43.bmp"
        ),
    },
    UserAccountPicture {
        name: "usertile44.bmp",
        bytes: include_bytes!(
            "../assets/windows7/User Account Pictures/Default Pictures/usertile44.bmp"
        ),
    },
];

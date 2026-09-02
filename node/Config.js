// Please tell me if I'm making way too many JavaScript files.
// But this one is for just adding constants into!

if (process.env.DELTAMOD_TEST === '1') {
    require('./TestIpcScope').installTestIpcScope();
}

const PARTITION = "persist:deltamod";

module.exports = {
    PARTITION
}

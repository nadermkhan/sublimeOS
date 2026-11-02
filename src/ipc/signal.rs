#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Signal {
    SIGHUP = 1,
    SIGINT = 2,
    SIGQUIT = 3,
    SIGILL = 4,
    SIGTRAP = 5,
    SIGABRT = 6,
    SIGBUS = 7,
    SIGFPE = 8,
    SIGKILL = 9,
    SIGUSR1 = 10,
    SIGSEGV = 11,
    SIGUSR2 = 12,
    SIGPIPE = 13,
    SIGALRM = 14,
    SIGTERM = 15,
    SIGCHLD = 17,
    SIGCONT = 18,
    SIGSTOP = 19,
    SIGTSTP = 20,
}

impl Signal {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            1 => Some(Signal::SIGHUP),
            2 => Some(Signal::SIGINT),
            3 => Some(Signal::SIGQUIT),
            4 => Some(Signal::SIGILL),
            5 => Some(Signal::SIGTRAP),
            6 => Some(Signal::SIGABRT),
            7 => Some(Signal::SIGBUS),
            8 => Some(Signal::SIGFPE),
            9 => Some(Signal::SIGKILL),
            10 => Some(Signal::SIGUSR1),
            11 => Some(Signal::SIGSEGV),
            12 => Some(Signal::SIGUSR2),
            13 => Some(Signal::SIGPIPE),
            14 => Some(Signal::SIGALRM),
            15 => Some(Signal::SIGTERM),
            17 => Some(Signal::SIGCHLD),
            18 => Some(Signal::SIGCONT),
            19 => Some(Signal::SIGSTOP),
            20 => Some(Signal::SIGTSTP),
            _ => None,
        }
    }

    pub fn bitmask(&self) -> u64 {
        1 << (*self as u8)
    }
}

pub struct SignalState {
    pub pending: u64,
    pub blocked: u64,
}

impl SignalState {
    pub const fn new() -> Self {
        Self {
            pending: 0,
            blocked: 0,
        }
    }

    pub fn send(&mut self, signal: Signal) {
        self.pending |= signal.bitmask();
    }

    pub fn take_pending(&mut self) -> Option<Signal> {
        let deliverable = self.pending & !self.blocked;
        if deliverable != 0 {
            let sig_num = deliverable.trailing_zeros() as u32;
            self.pending &= !(1 << sig_num);
            Signal::from_u32(sig_num)
        } else {
            None
        }
    }
}

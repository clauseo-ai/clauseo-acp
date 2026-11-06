//! The seam between the setup backend and S18's `Agents`: [`SetupAgents`], which the app's
//! `Agents` implements by forwarding and the tests implement by recording.

use std::time::Duration;

use clauseo_agents::{
    AccountFuture, Agents, Found, InstallState, Installed, Method, PasteCode, SigninError,
    SigninState,
};
use clauseo_protocol::AgentId;

/// What the backend asks of S18's `Agents`: a seam, so the tests record the calls.
pub(crate) trait SetupAgents: Send + Sync {
    /// `Agents::install`.
    fn install(&self, agent: &AgentId);
    /// `Agents::retry`.
    fn retry(&self, agent: &AgentId, use_system_proxy: bool);
    /// `Agents::state`.
    fn state(&self, agent: &AgentId) -> Option<InstallState>;
    /// `Agents::installed`.
    fn installed(&self, agent: &AgentId) -> Option<Installed>;
    /// `Agents::sign_in`.
    fn sign_in(&self, agent: &AgentId, method: Method) -> Result<(), SigninError>;
    /// `Agents::cancel_sign_in`.
    fn cancel_sign_in(&self, agent: &AgentId) -> Result<(), SigninError>;
    /// `Agents::paste_code`.
    fn paste_code(&self, agent: &AgentId, code: PasteCode) -> Result<(), SigninError>;
    /// `Agents::sign_out`.
    fn sign_out(&self, agent: &AgentId) -> Result<(), SigninError>;
    /// `Agents::check_status`.
    fn check_status(&self, agent: &AgentId) -> Result<(), SigninError>;
    /// `Agents::find_login`.
    fn find_login(&self, agent: &AgentId) -> Result<(), SigninError>;
    /// `Agents::use_found_login`.
    fn use_found_login(&self, agent: &AgentId) -> Result<(), SigninError>;
    /// `Agents::use_different_account`.
    fn use_different_account(&self, agent: &AgentId) -> Result<(), SigninError>;
    /// `Agents::signin_state`.
    fn signin_state(&self, agent: &AgentId) -> Option<SigninState>;
    /// `Agents::sign_in_going`.
    fn sign_in_going(&self, agent: &AgentId) -> bool;
    /// `Agents::stop_using_login`: no-reuse (unless `why` is a reason reuse stopped) and the
    /// home's link removed, in one job, which reports `SignedOut`.
    fn stop_using_login(&self, agent: &AgentId, why: Option<Found>) -> Result<(), SigninError>;
    /// `Agents::install_tries`.
    fn install_tries(&self, agent: &AgentId) -> Option<u32>;
    /// `Agents::shutdown`.
    fn shutdown(&self, wait: Duration) -> bool;
    /// `Agents::read_account`: R17's identity check.
    fn read_account(&self, agent: &AgentId) -> AccountFuture;
}

impl SetupAgents for Agents {
    fn install(&self, agent: &AgentId) {
        Agents::install(self, agent);
    }
    fn retry(&self, agent: &AgentId, use_system_proxy: bool) {
        Agents::retry(self, agent, use_system_proxy);
    }
    fn state(&self, agent: &AgentId) -> Option<InstallState> {
        Agents::state(self, agent)
    }
    fn installed(&self, agent: &AgentId) -> Option<Installed> {
        Agents::installed(self, agent)
    }
    fn sign_in(&self, agent: &AgentId, method: Method) -> Result<(), SigninError> {
        Agents::sign_in(self, agent, method)
    }
    fn cancel_sign_in(&self, agent: &AgentId) -> Result<(), SigninError> {
        Agents::cancel_sign_in(self, agent)
    }
    fn paste_code(&self, agent: &AgentId, code: PasteCode) -> Result<(), SigninError> {
        Agents::paste_code(self, agent, code)
    }
    fn sign_out(&self, agent: &AgentId) -> Result<(), SigninError> {
        Agents::sign_out(self, agent)
    }
    fn check_status(&self, agent: &AgentId) -> Result<(), SigninError> {
        Agents::check_status(self, agent)
    }
    fn find_login(&self, agent: &AgentId) -> Result<(), SigninError> {
        Agents::find_login(self, agent)
    }
    fn use_found_login(&self, agent: &AgentId) -> Result<(), SigninError> {
        Agents::use_found_login(self, agent)
    }
    fn use_different_account(&self, agent: &AgentId) -> Result<(), SigninError> {
        Agents::use_different_account(self, agent)
    }
    fn sign_in_going(&self, agent: &AgentId) -> bool {
        Agents::sign_in_going(self, agent)
    }
    fn signin_state(&self, agent: &AgentId) -> Option<SigninState> {
        Agents::signin_state(self, agent)
    }
    fn stop_using_login(&self, agent: &AgentId, why: Option<Found>) -> Result<(), SigninError> {
        Agents::stop_using_login(self, agent, why)
    }
    fn install_tries(&self, agent: &AgentId) -> Option<u32> {
        Agents::install_tries(self, agent)
    }
    fn shutdown(&self, wait: Duration) -> bool {
        Agents::shutdown(self, wait)
    }
    fn read_account(&self, agent: &AgentId) -> AccountFuture {
        Agents::read_account(self, agent)
    }
}

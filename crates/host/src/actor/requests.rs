//! The requests of handles: commands, queries, searches, subscriptions, acks, the desktop's
//! registrations and lookups, and shutdown, each checked against its principal.

use clauseo_protocol::{FolderRefId, Principal};

use super::input::Request;
use super::{
    Actor, HostPhase, app, commands, limits, locate, papers, query, settings, shutdown, subs, turn,
};
use crate::principal::{Access, authorize};

/// One request from a handle: dropped (answering `internal`) unless the host serves, but a
/// shutdown; otherwise routed to its module.
pub(super) fn on_request(actor: &mut Actor, request: Request) {
    if !matches!(actor.phase, HostPhase::Serving) {
        if let Request::Shutdown { reply } = request {
            shutdown::begin_shutdown(actor, Some(reply));
        } else {
            // Dropping the reply answers `internal`: the host is stopping.
            drop(request);
        }
        return;
    }
    match request {
        Request::Dispatch { command, reply } => commands::dispatch(actor, command, reply),
        Request::Query {
            principal,
            query,
            reply,
        } => query::query(actor, &principal, &query, reply),
        Request::Search {
            principal,
            window,
            request,
            reply,
        } => match authorize(&principal, Access::Query) {
            Ok(()) => actor.search.query(
                &window,
                request,
                Box::new(move |answer| {
                    let _gone = reply.send(answer);
                }),
            ),
            Err(error) => {
                let _gone = reply.send(Err(error));
            }
        },
        Request::Subscribe {
            principal,
            stream,
            after,
            reply,
        } => subs::subscribe(actor, &principal, stream, after, reply),
        Request::SubscribeEvents {
            principal,
            stream,
            after_seq,
            reply,
        } => subs::subscribe_events(actor, &principal, stream, after_seq, reply),
        Request::SubscribeApp { principal, reply } => app::subscribe(actor, &principal, reply),
        Request::SubscribeLimits { principal, reply } => {
            limits::subscribe(actor, &principal, reply);
        }
        Request::Ack {
            principal,
            stream,
            epoch,
            cursor,
            reply,
        } => {
            let acked = authorize(&principal, Access::Ack).map(|()| {
                subs::ack(actor, stream, epoch, cursor);
            });
            let _gone = reply.send(acked);
        }
        Request::RegisterFolder { path, reply } => {
            let minted = authorize(&Principal::LocalDesktop, Access::RegisterFolder)
                .and_then(|()| actor.ids.mint::<FolderRefId>(actor.clock.now().wall));
            let _gone = reply.send(minted.inspect(|id| {
                actor.folders.insert(*id, path);
            }));
        }
        Request::RegisterFile { path, reply } => {
            let minted = authorize(&Principal::LocalDesktop, Access::RegisterFile)
                .and_then(|()| papers::file_ref(actor, path));
            let _gone = reply.send(minted);
        }
        Request::LocateFile {
            principal,
            file,
            reply,
        } => {
            let located = authorize(&principal, Access::LocateFile)
                .and_then(|()| locate::locate_file(actor, file));
            let _gone = reply.send(located);
        }
        Request::MatterFolder {
            principal,
            matter,
            reply,
        } => {
            let folder = authorize(&principal, Access::MatterFolder)
                .and_then(|()| locate::matter_folder(actor, matter));
            let _gone = reply.send(folder);
        }
        Request::SetAgentLaunch {
            agent,
            launch,
            reply,
        } => {
            let written = settings::set_agent_launch(actor, &agent, &launch);
            let _gone = reply.send(written);
        }
        Request::SetPlan { agent, plan, reply } => {
            let written = settings::set_plan(actor, &agent, plan);
            let _gone = reply.send(written);
        }
        Request::SignIn(request) => turn::sign_in_request(actor, request),
        Request::Shutdown { reply } => {
            match authorize(&Principal::LocalDesktop, Access::Shutdown) {
                Ok(()) => shutdown::begin_shutdown(actor, Some(reply)),
                Err(error) => {
                    let _gone = reply.send(Err(error));
                }
            }
        }
    }
}

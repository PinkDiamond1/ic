// The valiadator executor provides non blocking access to the crypto services needed in the http handler.
use crate::{common::validation_error_to_http_error, HttpError};
use futures::FutureExt;
use http::StatusCode;
use ic_interfaces::crypto::IngressSigVerifier;
use ic_logger::ReplicaLogger;
use ic_types::{
    malicious_flags::MaliciousFlags,
    messages::{HttpRequest, HttpRequestContent, SignedIngress},
    time::current_time,
    RegistryVersion,
};
use ic_validator::{get_authorized_canisters, validate_request, CanisterIdSet};
use std::future::Future;
use std::sync::Arc;
use threadpool::ThreadPool;
use tokio::sync::oneshot;

// Number of threads used for the ingress validator executor.
const VALIDATOR_EXECUTOR_THREADS: usize = 1;

#[derive(Clone)]
pub(crate) struct ValidatorExecutor {
    validator: Arc<dyn IngressSigVerifier + Send + Sync>,
    threadpool: ThreadPool,
    logger: ReplicaLogger,
}

impl ValidatorExecutor {
    pub fn new(
        validator: Arc<dyn IngressSigVerifier + Send + Sync>,
        logger: ReplicaLogger,
    ) -> Self {
        ValidatorExecutor {
            validator,
            threadpool: ThreadPool::new(VALIDATOR_EXECUTOR_THREADS),
            logger,
        }
    }

    pub fn validate_signed_ingress(
        &self,
        request: SignedIngress,
        registry_version: RegistryVersion,
        malicious_flags: MaliciousFlags,
    ) -> impl Future<Output = Result<(), HttpError>> {
        let (tx, rx) = oneshot::channel();

        let message_id = request.id();
        let validator = self.validator.clone();
        self.threadpool.execute(move || {
            if !tx.is_closed() {
                let _ = tx.send(validate_request(
                    request.as_ref(),
                    validator.as_ref(),
                    current_time(),
                    registry_version,
                    &malicious_flags,
                ));
            }
        });
        let log = self.logger.clone();
        rx.map(move |v| match v {
            Err(recv_err) => Err(HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Internal Error: {:?}.", recv_err),
            }),
            Ok(Ok(())) => Ok(()),
            Ok(Err(val_err)) => Err(validation_error_to_http_error(message_id, val_err, &log)),
        })
    }

    pub fn get_authorized_canisters<C: HttpRequestContent + Clone + Send + Sync + 'static>(
        &self,
        request: HttpRequest<C>,
        registry_version: RegistryVersion,
        #[allow(unused_variables)] malicious_flags: MaliciousFlags,
    ) -> impl Future<Output = Result<CanisterIdSet, HttpError>> {
        let (tx, rx) = oneshot::channel();

        let message_id = request.id();
        let validator = self.validator.clone();
        self.threadpool.execute(move || {
            if !tx.is_closed() {
                let _ = tx.send(get_authorized_canisters(
                    &request,
                    validator.as_ref(),
                    current_time(),
                    registry_version,
                    &malicious_flags,
                ));
            }
        });
        let log = self.logger.clone();
        rx.map(move |v| match v {
            Err(recv_err) => Err(HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Internal Error: {:?}.", recv_err),
            }),
            Ok(Ok(canister_id_set)) => Ok(canister_id_set),
            Ok(Err(val_err)) => Err(validation_error_to_http_error(message_id, val_err, &log)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_request, validation_error_to_http_error, ValidatorExecutor};
    use ic_logger::replica_logger::no_op_logger;
    use ic_test_utilities::{
        crypto::temp_crypto_component_with_fake_registry,
        types::{
            ids::{canister_test_id, node_test_id},
            messages::SignedIngressBuilder,
        },
    };
    use ic_types::malicious_flags::MaliciousFlags;
    use ic_types::time::current_time;
    use ic_types::RegistryVersion;
    use ic_types::{
        messages::{
            Blob, HttpQueryContent, HttpRequest, HttpRequestEnvelope, HttpUserQuery, UserQuery,
        },
        time::expiry_time_from_now,
    };
    use ic_validator::get_authorized_canisters;
    use std::convert::TryFrom;
    use std::sync::Arc;

    #[tokio::test]
    async fn async_get_authorized_canisters() {
        let expiry_time = expiry_time_from_now();
        let content = HttpQueryContent::Query {
            query: HttpUserQuery {
                canister_id: Blob(vec![67, 3]),
                method_name: "foo".to_string(),
                arg: Blob(vec![23, 19, 4]),
                sender: Blob(vec![4]), // the anonymous user.
                nonce: None,
                ingress_expiry: expiry_time.as_nanos_since_unix_epoch(),
            },
        };
        let request = HttpRequestEnvelope::<HttpQueryContent> {
            content,
            sender_sig: Some(Blob(vec![])),
            sender_pubkey: Some(Blob(vec![])),
            sender_delegation: None,
        };
        let request = HttpRequest::<UserQuery>::try_from(request).unwrap();
        let sig_verifier = Arc::new(temp_crypto_component_with_fake_registry(node_test_id(0)));
        let validator = ValidatorExecutor::new(sig_verifier.clone(), no_op_logger());

        assert_eq!(
            validator
                .get_authorized_canisters(
                    request.clone(),
                    RegistryVersion::from(0),
                    MaliciousFlags::default()
                )
                .await,
            get_authorized_canisters(
                &request,
                sig_verifier.as_ref(),
                current_time(),
                RegistryVersion::from(0),
                &MaliciousFlags::default()
            )
            .map_err(|val_err| validation_error_to_http_error(
                request.id(),
                val_err,
                &no_op_logger()
            ))
        )
    }

    #[tokio::test]
    async fn async_validate_signed_ingress() {
        let request = SignedIngressBuilder::new()
            .canister_id(canister_test_id(420))
            .nonce(42)
            .build();
        let sig_verifier = Arc::new(temp_crypto_component_with_fake_registry(node_test_id(0)));
        let validator = ValidatorExecutor::new(sig_verifier.clone(), no_op_logger());

        assert_eq!(
            validator
                .validate_signed_ingress(
                    request.clone(),
                    RegistryVersion::from(0),
                    MaliciousFlags::default()
                )
                .await,
            validate_request(
                request.as_ref(),
                sig_verifier.as_ref(),
                current_time(),
                RegistryVersion::from(0),
                &MaliciousFlags::default()
            )
            .map_err(|val_err| validation_error_to_http_error(
                request.id(),
                val_err,
                &no_op_logger()
            ))
        )
    }
}

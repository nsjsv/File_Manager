use file_search::{
    default_socket_path, search_directory_fallback, search_via_socket, SearchError,
    SearchExcludeRules, SearchProviderFailure, SearchQuery,
};
use iced::futures::SinkExt;
use iced::Task;
use tokio_util::sync::CancellationToken;

use crate::model::{DirectoryFallbackCompletion, IndexedSearchOutcome, Message};

const DIRECTORY_FALLBACK_STREAM_CAPACITY: usize = 8;
const DIRECTORY_FALLBACK_WORKER_CAPACITY: usize = 4;

pub(crate) fn search_command(generation: u64, query: SearchQuery) -> Task<Message> {
    Task::perform(search_once(query), move |outcome| {
        Message::SearchResultsLoaded(generation, outcome)
    })
}

pub(crate) fn directory_fallback_search_command(
    generation: u64,
    query: SearchQuery,
    rules: SearchExcludeRules,
    cancellation: CancellationToken,
) -> Task<Message> {
    Task::stream(iced::stream::channel(
        DIRECTORY_FALLBACK_STREAM_CAPACITY,
        async move |mut output| {
            let (batch_sender, mut batch_receiver) =
                tokio::sync::mpsc::channel(DIRECTORY_FALLBACK_WORKER_CAPACITY);
            let worker_cancellation = cancellation.clone();
            let fallback_worker = tokio::task::spawn_blocking(move || {
                search_directory_fallback(&query, &rules, &worker_cancellation, |hits| {
                    if batch_sender.blocking_send(hits).is_err() {
                        worker_cancellation.cancel();
                    }
                })
            });

            while let Some(hits) = batch_receiver.recv().await {
                if output
                    .send(Message::SearchDirectoryBatchLoaded(generation, hits))
                    .await
                    .is_err()
                {
                    cancellation.cancel();
                    batch_receiver.close();
                    break;
                }
            }
            drop(batch_receiver);

            let completion = match fallback_worker.await {
                Ok(Ok(())) => DirectoryFallbackCompletion::Completed,
                Ok(Err(SearchError::Cancelled)) => DirectoryFallbackCompletion::Cancelled,
                Ok(Err(error)) => DirectoryFallbackCompletion::Failed(error.to_string()),
                Err(error) => DirectoryFallbackCompletion::Failed(format!(
                    "directory fallback worker failed: {error}"
                )),
            };
            let _ = output
                .send(Message::SearchDirectoryFinished(generation, completion))
                .await;
        },
    ))
}

async fn search_once(query: SearchQuery) -> IndexedSearchOutcome {
    match search_via_socket(&default_socket_path(), query).await {
        Ok(batch) => IndexedSearchOutcome::Batch(batch),
        Err(error) => indexed_outcome_from_error(error),
    }
}

fn indexed_outcome_from_error(error: SearchError) -> IndexedSearchOutcome {
    match error {
        error @ (SearchError::ProtocolIo(_)
        | SearchError::Json(_)
        | SearchError::ProtocolFrameTooLarge(_)) => {
            IndexedSearchOutcome::TransportUnavailable(error.to_string())
        }
        SearchError::SearchFailed { failure, .. } => match failure {
            SearchProviderFailure::Unavailable { message } => {
                IndexedSearchOutcome::ProviderUnavailable(message)
            }
            SearchProviderFailure::InvalidQuery { message } => {
                IndexedSearchOutcome::InvalidQuery(message)
            }
            SearchProviderFailure::Fatal { message } => IndexedSearchOutcome::Fatal(message),
        },
        SearchError::InvalidQuery(message) => IndexedSearchOutcome::InvalidQuery(message),
        error => IndexedSearchOutcome::Fatal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use file_search::{SearchError, SearchProviderFailure};

    use super::indexed_outcome_from_error;
    use crate::model::IndexedSearchOutcome;

    #[test]
    fn structured_unavailable_is_preserved_for_provider_routing() {
        let outcome = indexed_outcome_from_error(SearchError::SearchFailed {
            query_id: 7,
            failure: SearchProviderFailure::Unavailable {
                message: "index is starting".to_owned(),
            },
        });
        assert_eq!(
            outcome,
            IndexedSearchOutcome::ProviderUnavailable("index is starting".to_owned())
        );
    }

    #[test]
    fn malformed_protocol_is_transport_unavailable_before_provider_selection() {
        let outcome = indexed_outcome_from_error(SearchError::ProtocolFrameTooLarge(8));

        assert!(matches!(
            outcome,
            IndexedSearchOutcome::TransportUnavailable(_)
        ));
    }
}

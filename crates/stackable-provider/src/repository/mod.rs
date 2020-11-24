use crate::repository::package::Package;
use crate::repository::stackablerepository::StackableRepoProvider;
use kube::{Client, Api};
use crate::error::StackableError;
use kube::api::ListParams;
use std::convert::TryFrom;
use log::{trace, debug, info, error};
use crate::repository::repository::Repository;
pub mod package;
pub mod repository;
pub mod stackablerepository;

pub async fn find_repository(client: Client, package: &Package, repository_reference: Option<String>) -> Result<Option<StackableRepoProvider>, StackableError> {
    let repositories: Api<Repository> = Api::namespaced(client.clone(), "default");
    if let Some(repository_name) = repository_reference {
        // A repository name was provided, just check that exact repository for the package
        let repo = repositories.get(&repository_name).await?;
        let mut repo = StackableRepoProvider::try_from(&repo)?;
        if repo.provides_package(package.clone()).await? {
            return Ok(Some(repo));
        } else {
            return Ok(None);
        }
    } else {
        // No repository name was provided, retrieve all repositories from the orchestrator/apiserver
        // and check which one provides the package
        let list_params = ListParams::default();
        let repos = repositories.list(&list_params).await?;
        for repository in repos.iter() {
            let repo: &Repository = repository;
            debug!("got repo definition: {:?}", repository);
            // Convert repository to object implementing our trait
            // TODO: add generic implementation here to support different types of repository
            let mut repo = StackableRepoProvider::try_from(repository)?;
            trace!("converted to stackable repo: {:?}", repository);
            if repo.provides_package(package.clone()).await? {
                debug!("Found package {} in repository {}", &package, repo);
                return Ok(Some(repo));
            } else {
                debug!("Package {} not provided by repository {}", &package, repo);
            }
        }
    }
    Ok(None)
}
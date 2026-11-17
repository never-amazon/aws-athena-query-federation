use async_trait::async_trait;
use aws_sdk_glue::operation::get_databases::{GetDatabasesInput, GetDatabasesOutput};
use aws_sdk_glue::operation::get_table::{GetTableInput, GetTableOutput};
use aws_sdk_glue::operation::get_tables::{GetTablesInput, GetTablesOutput};

use crate::security::{
    send_with_override, AwsRequestOverrideConfiguration, AwsService, AwsServiceError,
};

#[async_trait]
pub trait GlueClient: Send + Sync {
    async fn get_databases(
        &self,
        request: GetDatabasesInput,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<GetDatabasesOutput, AwsServiceError>;

    async fn get_tables(
        &self,
        request: GetTablesInput,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<GetTablesOutput, AwsServiceError>;

    async fn get_table(
        &self,
        request: GetTableInput,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<GetTableOutput, AwsServiceError>;
}

#[async_trait]
impl GlueClient for aws_sdk_glue::Client {
    async fn get_databases(
        &self,
        request: GetDatabasesInput,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<GetDatabasesOutput, AwsServiceError> {
        let builder = self
            .get_databases()
            .set_catalog_id(request.catalog_id)
            .set_next_token(request.next_token)
            .set_max_results(request.max_results);
        send_with_override!(
            builder,
            override_configuration,
            aws_sdk_glue::config::Builder
        )
        .map_err(|e| AwsServiceError::from_sdk_error(AwsService::Glue, &e))
    }

    async fn get_tables(
        &self,
        request: GetTablesInput,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<GetTablesOutput, AwsServiceError> {
        let builder = self
            .get_tables()
            .set_catalog_id(request.catalog_id)
            .set_database_name(request.database_name)
            .set_expression(request.expression)
            .set_next_token(request.next_token)
            .set_max_results(request.max_results);
        send_with_override!(
            builder,
            override_configuration,
            aws_sdk_glue::config::Builder
        )
        .map_err(|e| AwsServiceError::from_sdk_error(AwsService::Glue, &e))
    }

    async fn get_table(
        &self,
        request: GetTableInput,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<GetTableOutput, AwsServiceError> {
        let builder = self
            .get_table()
            .set_catalog_id(request.catalog_id)
            .set_database_name(request.database_name)
            .set_name(request.name);
        send_with_override!(
            builder,
            override_configuration,
            aws_sdk_glue::config::Builder
        )
        .map_err(|e| AwsServiceError::from_sdk_error(AwsService::Glue, &e))
    }
}

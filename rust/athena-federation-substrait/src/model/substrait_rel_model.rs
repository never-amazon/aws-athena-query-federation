use crate::error::{Result, SubstraitError};
use crate::proto::{FetchRel, FilterRel, ProjectRel, ReadRel, Rel, SortRel};
use crate::substrait_rel_utils::SubstraitRelUtils;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SubstraitRelModel {
    read: Option<ReadRel>,
    filter: Option<FilterRel>,
    project: Option<ProjectRel>,
    sort: Option<SortRel>,
    fetch: Option<FetchRel>,
}

impl SubstraitRelModel {
    pub fn new(
        read_rel: Option<ReadRel>,
        filter_rel: Option<FilterRel>,
        project_rel: Option<ProjectRel>,
        sort_rel: Option<SortRel>,
        fetch_rel: Option<FetchRel>,
    ) -> SubstraitRelModel {
        SubstraitRelModel {
            read: read_rel,
            filter: filter_rel,
            project: project_rel,
            sort: sort_rel,
            fetch: fetch_rel,
        }
    }

    pub fn get_read_rel(&self) -> Option<&ReadRel> {
        self.read.as_ref()
    }

    pub fn get_filter_rel(&self) -> Option<&FilterRel> {
        self.filter.as_ref()
    }

    pub fn get_project_rel(&self) -> Option<&ProjectRel> {
        self.project.as_ref()
    }

    pub fn get_sort_rel(&self) -> Option<&SortRel> {
        self.sort.as_ref()
    }

    pub fn get_fetch_rel(&self) -> Option<&FetchRel> {
        self.fetch.as_ref()
    }

    pub fn build_substrait_rel_model(rel: Option<&Rel>) -> Result<SubstraitRelModel> {
        let rel = rel
            .ok_or_else(|| SubstraitError::illegal_argument("Substrait relation cannot be null"))?;
        Ok(SubstraitRelModel::new(
            SubstraitRelUtils::get_read_rel(Some(rel)).cloned(),
            SubstraitRelUtils::get_filter_rel(Some(rel)).cloned(),
            SubstraitRelUtils::get_project_rel(Some(rel)).cloned(),
            SubstraitRelUtils::get_sort_rel(Some(rel)).cloned(),
            SubstraitRelUtils::get_fetch_rel(Some(rel)).cloned(),
        ))
    }
}

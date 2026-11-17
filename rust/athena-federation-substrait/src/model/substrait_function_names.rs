pub struct SubstraitFunctionNames;

impl SubstraitFunctionNames {
    pub const NOT_BOOL: &'static str = "not:bool";
    pub const AND_BOOL: &'static str = "and:bool";
    pub const OR_BOOL: &'static str = "or:bool";
    pub const GT_ANY_ANY: &'static str = "gt:any_any";
    pub const GT_PTS_PTS: &'static str = "gt:pts_pts";
    pub const GTE_ANY_ANY: &'static str = "gte:any_any";
    pub const GTE_PTS_PTS: &'static str = "gte:pts_pts";
    pub const LT_ANY_ANY: &'static str = "lt:any_any";
    pub const LT_PTS_PTS: &'static str = "lt:pts_pts";
    pub const LTE_ANY_ANY: &'static str = "lte:any_any";
    pub const LTE_PTS_PTS: &'static str = "lte:pts_pts";
    pub const EQUAL_ANY_ANY: &'static str = "equal:any_any";
    pub const NOT_EQUAL_ANY_ANY: &'static str = "not_equal:any_any";
    pub const IS_NULL_ANY: &'static str = "is_null:any";
    pub const IS_NOT_NULL_ANY: &'static str = "is_not_null:any";
}

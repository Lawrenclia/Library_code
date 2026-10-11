//! Shared application composition. Capability registration never grants a write.
use crate::{Failure, Result};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Clone, Copy, Serialize)]
pub struct BrowserSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub url: &'static str,
    pub profile: &'static str,
}
pub const BROWSERS: &[BrowserSpec] = &[
    BrowserSpec {
        id: "wos",
        label: "WOS · 机构访问与元数据",
        url: "https://webofscience.clarivate.cn/wos/",
        profile: "wos-profile",
    },
    BrowserSpec {
        id: "sa",
        label: "机构库 · SA 比对",
        url: "http://admin.ir.lib.sjtu.edu.cn/#/dataCompare/list",
        profile: "library-profile",
    },
    BrowserSpec {
        id: "library",
        label: "机构库前端 · 成果检索",
        url: "http://www.ir.lib.sjtu.edu.cn/advancedSearch",
        profile: "library-profile",
    },
    BrowserSpec {
        id: "import",
        label: "机构库 · 导入与批次管理",
        url: "http://admin.ir.lib.sjtu.edu.cn/#/collectItem/batchManage",
        profile: "library-profile",
    },
    BrowserSpec {
        id: "scholar",
        label: "机构库 · 学者管理与别名",
        url: "http://admin.ir.lib.sjtu.edu.cn/#/scholar/list",
        profile: "library-profile",
    },
    BrowserSpec {
        id: "duplicate",
        label: "机构库 · 重复数据管理",
        url: "http://admin.ir.lib.sjtu.edu.cn/#/collectItem/duplicateData",
        profile: "library-profile",
    },
];
pub const DATABASE_DIRECTORY: &str = "https://www.lib.sjtu.edu.cn/f/database/database.shtml";
pub fn browser(id: &str) -> Result<&'static BrowserSpec> {
    BROWSERS
        .iter()
        .find(|b| b.id == id)
        .ok_or_else(|| Failure::new("INVALID_CHANNEL", "未知浏览器通道。"))
}

pub fn manifest() -> Value {
    let operations: Vec<Value> = crate::workflow::StepAction::ALL
        .iter()
        .map(|a| json!({"id":a.id(),"service":a.service(),"kind":a.kind()}))
        .collect();
    json!({
        "schema_version":1,
        "services":[
            {"id":"inputs","label":"名单与版本","description":"读取物理 A 列空白任务、负责人范围、原表快照与旧版迁移。","state":"implemented","limitation":"旧分类、准备资料与支持的旧检查点严格绑定；缺少原回执的旧操作保留待核验，不生成虚构成功结果。"},
            {"id":"downloads","label":"来源与原始下载","description":"WOS/CNKI 专用获取、通用渠道检索导出、批量队列、详情字段和原生下载回执。","state":"implemented","limitation":"通用驱动按保存的入口、实际控件或渠道步骤执行；不能确认完整结果或原导出时停止并保留来源。各站实际兼容性待自测。"},
            {"id":"library","label":"本库查询与四分支","description":"正确题名及标识符查库，确认非交大、已有、缺失或未查询到。","state":"implemented","limitation":"未查询到分支已接入独立备注保存与回读，保留待处理；实际窗口契约和业务查询待自测。"},
            {"id":"sa","label":"SA 逐项核验","description":"按匹配数分流，保存每个差异的来源、结论与实时回读。","state":"implemented","limitation":"完成前仍需本条准确 SA 回读。"},
            {"id":"duplicates","label":"重复条目","description":"选择主条目与被合并条目，核对保留字段后合并并回读。","state":"implemented","limitation":"真实平台合并与当前版本恢复场景待自测，不以点击成功代替最终回读。"},
            {"id":"metadata","label":"作者与单位字段","description":"按原始证据编辑 PPT 所需角色和完整署名顺序，保留其他元数据。","state":"implemented","limitation":"执行须有对应的实际页面控件和完整回读；不猜写缺少来源的作者角色或单位。"},
            {"id":"authors","label":"认领与别名","description":"按完整工号读取身份，明确选择作者，必要时增加有来源的别名。","state":"implemented","limitation":"真实机构业务验收待完成。"},
            {"id":"materials","label":"模板与材料","description":"注册实际 Excel 模板，统一保存单篇与批量材料、字段校验、导出副本和来源表。","state":"implemented","limitation":"原始导出优先；单篇统一归档和提交选择为新增代码，暂未验证，字段通过仍需业务核对。"},
            {"id":"ai","label":"AI 分类与补全","description":"调用配置的 API，验证结构化结果和每个字段的来源引用。","state":"implemented","limitation":"需配置 API；分类不决定归属或平台完成状态。"},
            {"id":"submission","label":"上传、导入与推送","description":"按 PPT 分阶段执行，绑定 SA 说明与五项推送规则并回读。","state":"implemented","limitation":"原文件与模板均接入按渠道提交、单篇核验、冻结材料和跨名单原批次回读；实际后台窗口兼容性待自测。"},
            {"id":"storage","label":"记录、恢复与报告","description":"SQLite 保存原意图、来源队列与下载，恢复先回读，导出全字段来源 Excel。","state":"implemented","limitation":"没有原生下载完成或平台结果回执时保留未知状态；人工核对后可取消原来源导出记录，文件与历史回执保留。"}
        ],
        "browsers":BROWSERS,
        "channels":crate::catalog::channels(),
        "metadata_providers":[{"id":"crossref","label":"Crossref DOI 登记元数据","lookup":"implemented","key":"doi","archive_format":"json","import_channel":null,"live_verified":false}],
        "operations":operations,
        "flows":[
            {"id":"zero","label":"匹配 0 条","steps":["取得原始来源","核对论文身份与交大归属","用正确题名和标识符查本库","确认分支"],"outcomes":["非交大：有依据后备注并处理","本库已有：关联实际条目后逐项核验","本库缺失：准备材料，再上传、导入、推送","未查询到：记录检索范围，确认独立备注保存并回读，保留未处理"]},
            {"id":"existing","label":"匹配 1 条","steps":["读取当前 SA 与完整本库记录","逐项核对待处理原因","必要时修改字段或认领","重新回读后完成 SA"],"outcomes":["每个原因分别保存来源与结论","存在未解决差异时不能完成"]},
            {"id":"duplicate","label":"匹配 2 条及以上","steps":["读取重复候选完整字段","核实同一成果并选择主条目","确认保留字段后合并","回读候选、主条目与 SA","继续剩余原因核对"],"outcomes":["仍有第三条匹配时继续核对","合并成功不直接等于 SA 已处理"]}
        ],
        "rules":["原始导出优先，模板按实际必填要求准备","下载、上传、导入、推送、SA 完成分别记录","普通逐篇失败继续，通道故障保留队列等待恢复","写入结果未知先回读，禁止直接重发","登录由用户完成；页面已打开不表示有访问权限"],
        "live_verified":false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composition_covers_every_registered_operation_and_unique_browser() {
        let manifest = manifest();
        for action in crate::workflow::StepAction::ALL {
            assert!(manifest["services"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["id"] == action.service()));
        }
        let mut ids = std::collections::HashSet::new();
        for b in BROWSERS {
            assert!(ids.insert(b.id));
            let u = url::Url::parse(b.url).unwrap();
            assert!(matches!(u.scheme(), "http" | "https"));
            assert!(u.username().is_empty() && u.password().is_none());
            assert_eq!(browser(b.id).unwrap().profile, b.profile);
        }
        assert!(browser("unregistered").is_err());
        assert_eq!(manifest["live_verified"], false);
    }
    #[test]
    fn registered_channels_do_not_pretend_to_have_unsupported_drivers() {
        let c = crate::catalog::channels();
        for channel in c.as_array().unwrap() {
            if channel["id"] == "wos_txt" {
                assert_eq!(channel["capabilities"]["export"], "implemented");
            } else {
                assert_ne!(channel["capabilities"]["export"], "implemented");
                assert_ne!(channel["capabilities"]["submit"], "implemented");
            }
            assert_eq!(channel["live_verified"], false);
        }
    }
}

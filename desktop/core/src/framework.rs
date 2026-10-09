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
            {"id":"inputs","label":"名单与版本","description":"读取 Excel、负责人范围、原表快照与旧版迁移。","state":"partial","limitation":"旧版其他分类材料与部分历史恢复待补齐。"},
            {"id":"downloads","label":"来源与原始下载","description":"优先数据库导出；原生下载回执和本地 Excel、CSV、TXT 原文件接入。","state":"partial","limitation":"本地选择记录保留全部字段，不自动确认归属；其他数据库自动导出待接入，真实 WOS 下载待验收。"},
            {"id":"library","label":"本库查询与四分支","description":"正确题名及标识符查库，确认非交大、已有、缺失或未查询到。","state":"implemented","limitation":"真实登录后的动态页面和业务查询待验收。"},
            {"id":"sa","label":"SA 逐项核验","description":"按匹配数分流，保存每个差异的来源、结论与实时回读。","state":"implemented","limitation":"完成前仍需本条准确 SA 回读。"},
            {"id":"duplicates","label":"重复条目","description":"选择主条目与被合并条目，核对保留字段后合并并回读。","state":"partial","limitation":"完整进程重启合并验收及真实平台合并待验证。"},
            {"id":"metadata","label":"作者与单位字段","description":"按原始证据编辑角色和完整署名顺序，保留其他元数据。","state":"partial","limitation":"没有实际页面控件的其他字段需逐项适配。"},
            {"id":"authors","label":"认领与别名","description":"按完整工号读取身份，明确选择作者，必要时增加有来源的别名。","state":"implemented","limitation":"真实机构业务验收待完成。"},
            {"id":"materials","label":"模板与材料","description":"注册实际 Excel 模板，保留格式并校验必填内容。","state":"implemented","limitation":"必填缺项的材料保留待补充，不自动上传。"},
            {"id":"ai","label":"AI 分类与补全","description":"调用配置的 API，验证结构化结果和每个字段的来源引用。","state":"implemented","limitation":"需配置 API；分类不决定归属或平台完成状态。"},
            {"id":"submission","label":"上传、导入与推送","description":"按 PPT 分阶段执行，绑定 SA 说明与五项推送规则并回读。","state":"partial","limitation":"自动导入目前仅 WOS TXT；真实后台闭环待验收。"},
            {"id":"storage","label":"记录、恢复与报告","description":"SQLite 保存意图、进度与完整证据，恢复先回读，导出来源 Excel。","state":"partial","limitation":"部分旧操作和丢失上传窗口的独立恢复待补齐。"}
        ],
        "browsers":BROWSERS,
        "channels":crate::catalog::channels(),
        "operations":operations,
        "flows":[
            {"id":"zero","label":"匹配 0 条","steps":["取得原始来源","核对论文身份与交大归属","用正确题名和标识符查本库","确认分支"],"outcomes":["非交大：有依据后备注并处理","本库已有：关联实际条目后逐项核验","本库缺失：准备材料，再上传、导入、推送","未查询到：记录检索范围，保留未处理"]},
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

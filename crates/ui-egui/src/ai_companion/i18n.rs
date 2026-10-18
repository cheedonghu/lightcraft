//! Assistant-owned additions, isolated from upstream interface catalogs.
pub(super) fn tr(source: &str) -> &str {
    if crate::i18n::language() == crate::i18n::Locale::ZhHans {
        match source {
            "Assistant settings" => "助手设置",
            "AI service" => "AI 服务",
            "Checking connection…" => "正在检测连接…",
            "Connected" => "已连接",
            "Disconnected" => "未连接",
            "Connection not checked" => "尚未检测连接",
            "Conversation management" => "对话管理",
            "Manage the AI conversation for the selected photo." => "管理当前照片的 AI 对话记录。",
            "Diagnostics" => "诊断",
            "Processing history" => "处理过程",
            "Tool activity" => "工具调用",
            "Tool response recorded." => "工具返回内容已记录。",
            "Read photography guidance" => "读取摄影指导",
            "Read reference material" => "读取参考资料",
            "Read photo settings" => "读取照片状态",
            "Read available edit controls" => "读取可用调整参数",
            "Read photo preview" => "读取照片预览",
            "Read photo histogram" => "读取照片直方图",
            "Read manual masks" => "读取手动蒙版",
            "Sample photo colors" => "取样照片颜色",
            "Prepare photo adjustments" => "准备照片调整方案",
            "Use a photo tool" => "调用照片工具",
            "Review this photo" => "点评这张照片",
            "Naturally optimize" => "自然优化",
            "Please review this photo." => "请点评这张照片。",
            "Please naturally optimize this photo." => "请自然优化这张照片。",
            "What would you like to change?" => "想了解这张照片，或调整哪里？",
            "Send request" => "发送请求",
            "Write a request to send" => "输入要求后发送",
            "Start with your photo" => "从这张照片开始",
            "Choose a prompt below, or write your own request." => "选择下面的提示，或写下你的想法。",
            "Conversation" => "对话",
            _ => crate::i18n::tr(source),
        }
    } else {
        crate::i18n::tr(source)
    }
}

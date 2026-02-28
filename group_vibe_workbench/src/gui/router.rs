use std::collections::HashMap;

/// Router - 页面路由管理器
///
/// 管理应用的页面路由，支持页面切换、历史记录等功能
///
/// # 示例
/// ```rust
/// let mut router = Router::new();
/// router.register("home", Box::new(HomePage::new()));
/// router.register("editor", Box::new(EditorPage::new()));
///
/// // 导航到编辑器页面
/// router.navigate("editor");
///
/// // 返回上一页
/// router.back();
/// ```
pub struct Router {
    /// 注册的页面映射
    pages: HashMap<String, Box<dyn std::any::Any>>,
    /// 当前页面 ID
    current_page: Option<String>,
    /// 历史记录栈
    history: Vec<String>,
    /// 最大历史记录数
    max_history: usize,
}

impl Router {
    /// 创建一个新的路由器
    pub fn new() -> Self {
        Self {
            pages: HashMap::new(),
            current_page: None,
            history: Vec::new(),
            max_history: 50,
        }
    }

    /// 设置最大历史记录数
    pub fn with_max_history(mut self, max_history: usize) -> Self {
        self.max_history = max_history;
        self
    }

    /// 注册一个页面
    pub fn register(&mut self, id: impl Into<String>, page: Box<dyn std::any::Any>) {
        self.pages.insert(id.into(), page);
    }

    /// 导航到指定页面
    pub fn navigate(&mut self, page_id: impl Into<String>) -> bool {
        let page_id = page_id.into();

        // 检查页面是否存在
        if !self.pages.contains_key(&page_id) {
            log::warn!("Page not found: {}", page_id);
            return false;
        }

        // 保存当前页面到历史记录
        if let Some(current) = &self.current_page {
            self.history.push(current.clone());

            // 限制历史记录数量
            if self.history.len() > self.max_history {
                self.history.remove(0);
            }
        }

        // 切换到新页面
        self.current_page = Some(page_id);
        true
    }

    /// 返回上一页
    pub fn back(&mut self) -> bool {
        if let Some(previous) = self.history.pop() {
            self.current_page = Some(previous);
            true
        } else {
            false
        }
    }

    /// 前进到下一页（如果有的话）
    pub fn forward(&mut self) -> bool {
        // TODO: 实现前进功能需要维护两个栈
        false
    }

    /// 获取当前页面 ID
    pub fn current_page(&self) -> Option<&str> {
        self.current_page.as_deref()
    }

    /// 检查是否可以返回
    pub fn can_back(&self) -> bool {
        !self.history.is_empty()
    }

    /// 获取历史记录
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// 清空历史记录
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// 替换当前页面（不添加到历史记录）
    pub fn replace(&mut self, page_id: impl Into<String>) -> bool {
        let page_id = page_id.into();

        // 检查页面是否存在
        if !self.pages.contains_key(&page_id) {
            log::warn!("Page not found: {}", page_id);
            return false;
        }

        // 直接替换当前页面
        self.current_page = Some(page_id);
        true
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

/// RouteGuard - 路由守卫
///
/// 在页面切换前后执行的钩子函数
pub trait RouteGuard {
    /// 在离开当前页面前执行
    /// 返回 false 可以阻止页面切换
    fn before_leave(&self, from: &str, to: &str) -> bool {
        let _ = (from, to);
        true
    }

    /// 在进入新页面前执行
    /// 返回 false 可以阻止页面切换
    fn before_enter(&self, from: &str, to: &str) -> bool {
        let _ = (from, to);
        true
    }

    /// 在进入新页面后执行
    fn after_enter(&self, from: &str, to: &str) {
        let _ = (from, to);
    }
}

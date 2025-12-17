#[macro_export]
macro_rules! dog_adapter {
    ($ty:ty, $req:ty, $params:ty) => {
        #[async_trait::async_trait]
        impl $crate::DogService<$req, $params> for $ty {
            fn capabilities(&self) -> $crate::ServiceCapabilities {
                self.capabilities.clone()
            }

            async fn create(
                &self,
                ctx: &$crate::tenant::TenantContext,
                data: $req,
                params: $params,
            ) -> anyhow::Result<$req> {
                self._create(ctx, data, params).await
            }

            async fn find(
                &self,
                ctx: &$crate::tenant::TenantContext,
                params: $params,
            ) -> anyhow::Result<Vec<$req>> {
                self._find(ctx, params).await
            }

            async fn get(
                &self,
                ctx: &$crate::tenant::TenantContext,
                id: &str,
                params: $params,
            ) -> anyhow::Result<$req> {
                self._get(ctx, id, params).await
            }

            async fn update(
                &self,
                ctx: &$crate::tenant::TenantContext,
                id: &str,
                data: $req,
                params: $params,
            ) -> anyhow::Result<$req> {
                self._update(ctx, id, data, params).await
            }

            async fn patch(
                &self,
                ctx: &$crate::tenant::TenantContext,
                id: Option<&str>,
                data: $req,
                params: $params,
            ) -> anyhow::Result<$req> {
                self._patch(ctx, id, data, params).await
            }

            async fn remove(
                &self,
                ctx: &$crate::tenant::TenantContext,
                id: Option<&str>,
                params: $params,
            ) -> anyhow::Result<$req> {
                self._remove(ctx, id, params).await
            }
        }
    };
}

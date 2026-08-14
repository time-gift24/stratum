# Stratum Ontology

Stratum Ontology 是一个独立的 schema 管理领域，定义对象的类型、属性和关系；它不存储对象实例，也不承担推理。

## Language

**Application metamodel**:
Stratum 用来规定合法 Ontology 定义的封闭元模型；用户创建的 Ontology 是它的实例，未来业务对象则是 Object Type 的实例。元模型校验、Ontology 定义与对象实例是三个不同层次。
_Avoid_: 推理本体、把用户 Ontology 与对象实例混为一层

**Ontology**:
一个独立保存和校验的 schema 聚合，拥有自己的 Object Type、Property 与 Link Type。
_Avoid_: knowledge base、memory、graph database

**Ontology entity ID**:
Ontology、Object Type、Property 或 Link Type 的不可变、不透明且带实体类型的 UUIDv7 身份。服务生成 Ontology ID；客户端生成 Object Type、Property 与 Link Type ID。每种子实体 ID 分别在所有现存 Ontology 中全局唯一，引用只使用 ID。客户端 UUIDv7 生成器不得故意复用已删除的子实体 ID；MVP hard deletion 不保留 tombstone 或 identity registry，因此服务不承诺检测客户端故意重提历史 ID。
_Avoid_: RID、external ID、用名称作身份、服务端 tombstone 保证

**标识名 (name)**:
实体在所属范围内唯一、可修改的程序可读符号名；Ontology name 在部署内唯一，Object Type 与 Link Type name 分别在所属 Ontology 内唯一，Property name 只在所属 Object Type 内唯一。界面可称为“代码名称”，修改它不改变实体身份。长度为一至六十四位，匹配 `^[a-z][a-z0-9_]{0,63}$`。
_Avoid_: Machine name、apiName、slug、key

**Display name**:
面向人的可修改 Unicode 名称，不承担身份或唯一性。
_Avoid_: label、title（作实体名称义时）

**Property**:
由一个 Object Type 独占的标量字段定义；它声明值类型以及未来对象实例是否必须提供该值。
_Avoid_: Shared Property Type、可复用字段

**Property value type**:
Property 的标量值域，只包含 string、integer、number、boolean、date 与 date_time。可选 Property 表示值可以缺席，不表示 null 是合法值。
_Avoid_: nullable type、通用 JSON、隐式复杂类型

**Link Type**:
连接一个 source Object Type 与一个 target Object Type 的有名称二元关系；同一关系可从任一方向遍历，不创建独立的反向 Link Type。
_Avoid_: inverse link、物理外键绑定、join binding

**Physical data binding**:
把 Object Type、Property 或 Link Type 映射到具体数据源、列、外键或 join 结构的配置不属于 Ontology 元模型。
_Avoid_: 在 Link Type 中保存 fk property、join datasource 或实例存储细节

**Link cardinality**:
Link Type 分别声明 source-to-target 与 target-to-source 的最大基数。one 表示零或一个，many 表示零或多个；MVP 不表达必需关系。
_Avoid_: 含糊的 one-to-many 标签、exactly one

**Candidate schema**:
画布保存时提交的完整 Ontology 目标状态，而不是增量 patch。校验与持久化都以整张候选图为单位。
_Avoid_: patch、partial update、增量 schema

**Hard deletion**:
Candidate schema 中缺席的旧 Property、Link Type 或 Object Type 被永久移除；删除 Object Type 时，候选图也必须移除全部相关 Link Type。
_Avoid_: soft delete、deprecation、回收站

**Validation report**:
Candidate schema 的确定性完整校验结果，由稳定机器错误码、定位字段或实体的路径以及安全的人类可读消息组成。存在任一 violation 时整次保存不产生写入。
_Avoid_: first-error response、部分成功、自动修复

**Revision**:
Ontology 当前状态的单调递增并发标记，不属于 Ontology 文档或 schema 语义。它只支持条件保存，不表示可读取或回滚的历史版本。
_Avoid_: version、snapshot、history sequence

**Canvas layout**:
与 Ontology 一同持久化、但不参与 schema 语义的编辑器元数据；它只把 Object Type ID 映射到画布坐标。
_Avoid_: Object Type position、把布局当作领域属性

**Neighborhood query**:
从一个 Object Type 出发，沿 Link Type 向任一方向扩展至指定跳数所得的只读 schema 子图。它不是 Candidate schema，不能作为完整保存输入。
_Avoid_: radius search、地理半径、向量相似度、partial schema

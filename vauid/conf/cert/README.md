# 测试证书如何生成

```bash
cd /path/to/store/cert

openssl genpkey -algorithm RSA -out cert.key -pkeyopt rsa_keygen_bits:2048
openssl req -new -key cert.key -out cert.csr -subj "/C=CN/ST=beijing/L=beijing/O=tquic/CN=vauid.org"
openssl x509 -req -in cert.csr -signkey cert.key -out cert.crt
```

## 说明

在 `cert` 目录下生成的证书文件，用于测试 P2P 连接。

目前`cert`目录下已有测试证书可以直接使用

## 与 quic.conf.toml 的对应关系

生成的三个文件与 `[tls]` 配置字段的映射：

| 文件 | 用途 | 配置字段 |
| --- | --- | --- |
| `cert.crt` | PEM 证书 | `cert_file` |
| `cert.key` | PEM 私钥 | `key_file` |
| `cert.csr` | 签名请求（配置不使用） | - |

注意：配置文件中的证书路径是相对**应用运行目录**的，不是相对配置文件本身。
当前配置（从 `vauid/` 目录运行）写法为：

```toml
[tls]
cert_file = "conf/cert/cert.crt"
key_file = "conf/cert/cert.key"
```
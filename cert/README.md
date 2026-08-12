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
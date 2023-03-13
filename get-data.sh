#!/bin/sh
echo "download sing-box"
curl -Lo "sing-box.tar.gz" "https://github.com/SagerNet/sing-box/releases/download/v1.2-beta8/sing-box-1.2-beta8-linux-amd64v3.tar.gz"
tar -xvf sing-box.tar.gz
mv sing-box-1.2-beta8-linux-amd64v3/sing-box .
rm -r sing-box-1.2-beta8-linux-amd64v3 sing-box.tar.gz
mkdir data
echo "download cf-v4.txt"
curl -Lo "cf-v4.txt" https://raw.githubusercontent.com/plusls/cdn-ip-tester/master/cf-v4.txt
echo "download cf-v6.txt"
curl -Lo "cf-v6.txt" https://raw.githubusercontent.com/plusls/cdn-ip-tester/master/cf-v6.txt
echo "download outbound-template.json"
curl -Lo "data/outbound-template.json" https://raw.githubusercontent.com/plusls/cdn-ip-tester/master/outbound-template.json
echo "download sing-box-template.json"
curl -Lo "data/sing-box-template.json" https://raw.githubusercontent.com/plusls/cdn-ip-tester/master/sing-box-template.json
echo "download ip-tester.toml"
curl -Lo "data/ip-tester.toml" https://raw.githubusercontent.com/plusls/cdn-ip-tester/master/ip-tester.toml
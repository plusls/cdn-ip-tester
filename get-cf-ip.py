import hashlib
from dataclasses import dataclass
from enum import Enum, unique, auto
from typing import Optional, List
from urllib.parse import unquote

import requests
from bs4 import BeautifulSoup, Tag
from requests import Response


@dataclass(repr=True, init=True)
class QueryResult:
    @unique
    class ResultType(Enum):
        AS = auto()
        NET = auto()
        DNS = auto()
        IP = auto()

    result_type: ResultType
    result: str
    path: str
    description: str
    region: str


@dataclass(repr=True, init=True)
class AutonomousSystem:
    name: str
    region: str
    prefixes_v4: List[QueryResult]
    prefixes_v6: List[QueryResult]


class BgpInfo:
    DOMAIN = 'bgp.he.net'
    URL = f"https://{DOMAIN}"

    def __init__(self):
        self.session = requests.Session()
        self.session.headers["User-Agent"] = "Chrome"

        self.session.get(f"{self.URL}/search")
        path_cookie: Optional[str] = self.session.cookies.get('path', domain=self.DOMAIN)
        path_cookie = unquote(path_cookie)
        ip_res: Response = self.session.get(f"{self.URL}/i")
        ip: str = ip_res.text.strip()
        p = hashlib.md5(path_cookie.encode()).hexdigest()
        i = hashlib.md5(ip.encode()).hexdigest()

        r = self.session.post(f"{self.URL}/jc", data={'p': p, 'i': i})

        if r.status_code != 200:
            raise Exception(f"init error! p: {p}, i: {i}, r: {r} {r.content}, ip: {ip}, path_cookie: {path_cookie}")

    @staticmethod
    def _parse_result_table(table: Tag) -> List[QueryResult]:
        ret = []
        lines: List[Tag] = [line for line in table.tbody.children if type(line) is Tag]
        for line in lines:
            tds = line.find_all("td")
            assert (len(tds) == 2)
            href: str = tds[0].a.attrs['href']
            result: str = tds[0].a.get_text()
            description: str = tds[1].get_text().strip()
            region: str = ''
            result_type: QueryResult.ResultType
            img: Optional[Tag] = tds[1].find('img')
            if img is not None:
                region = img.attrs['title']

            if href.startswith('/dns/'):
                result_type = QueryResult.ResultType.DNS
            elif href.startswith('/AS'):
                result_type = QueryResult.ResultType.AS
            elif href.startswith('/net/'):
                result_type = QueryResult.ResultType.NET
            else:
                print(href, result, description, region)
                raise Exception('fuck')
            ret.append(QueryResult(result_type, result, href, description, region))
        # for i in ret[-30:]:
        #     print(i)
        return ret

    def search(self, text: str) -> List[QueryResult]:
        params = {
            'search[search]': text,
            'commit': 'Search'
        }

        r = self.session.get(f"{self.URL}/search", params=params)
        soup = BeautifulSoup(r.text, features="html.parser")
        soup_text = soup.get_text()

        if 'did not return any results.  You may go Back to the page that referred you' in soup_text:
            return []

        return self._parse_result_table(soup.find_all(attrs={'class': 'w100p'})[0])

    def autonomous_system(self, name: str) -> AutonomousSystem:
        assert (name.startswith("AS"))
        as_res = self.session.get(f"{self.URL}/{name}")
        as_res_soup = BeautifulSoup(as_res.text, features="html.parser")
        prefixes4_tables = as_res_soup.find_all(attrs={'id': 'table_prefixes4'})
        prefixes6_tables = as_res_soup.find_all(attrs={'id': 'table_prefixes6'})

        if prefixes4_tables:
            prefixes_v4 = self._parse_result_table(prefixes4_tables[0])
        else:
            prefixes_v4 = []

        if prefixes6_tables:
            prefixes_v6 = self._parse_result_table(prefixes6_tables[0])
        else:
            prefixes_v6 = []

        return AutonomousSystem(name, "", prefixes_v4, prefixes_v6)


def main():


    bgp_info = BgpInfo()
    bgp_info.search("cloudflare")
    as_list: List[AutonomousSystem] = [bgp_info.autonomous_system(result.result) for result in
                                       bgp_info.search("cloudflare") if
                                       result.result_type is QueryResult.ResultType.AS]

    with open('cf-v4.txt', 'w') as cf_v4, open('cf-v6.txt', 'w') as cf_v6:
        for as_info in as_list:
            for net in as_info.prefixes_v4:
                assert net.result_type is QueryResult.ResultType.NET
                cf_v4.write(f'{net.result}\n')
            for net in as_info.prefixes_v6:
                assert net.result_type is QueryResult.ResultType.NET
                cf_v6.write(f'{net.result}\n')
    print('update cf ip success!')

# res = requests.get("https://bgp.he.net/search", params=params, headers=headers)
#     print(res.text)
if __name__ == '__main__':
    main()
